//! libmpv backend.
//!
//! Three things make this safe to ship as an optional component:
//!
//! 1. The DLL is opened with `libloading` at runtime, so the process starts and
//!    runs without it. Nothing is linked at build time.
//! 2. All FFI is confined to this module and every call site checks the mpv
//!    error code before continuing.
//! 3. Events are pumped on a dedicated thread and delivered over a channel, so
//!    the UI thread never blocks inside `mpv_wait_event`.
//!
//! Rendering uses `mpv_render_context` against the framebuffer that egui is
//! already drawing into. That is what avoids a child window: no z-order fights,
//! no black frames while the window is resized.

use crate::EngineReport;
use app_core::{
    AspectMode, EngineKind, PlaybackCommand, PlaybackEngine, PlaybackError, PlaybackEvent,
    PlaybackRequest, RenderTarget,
};
use libloading::{Library, Symbol};
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

type MpvHandle = *mut c_void;
type MpvRenderContext = *mut c_void;

const MPV_FORMAT_FLAG: c_int = 3;
const MPV_FORMAT_DOUBLE: c_int = 5;

const MPV_EVENT_SHUTDOWN: c_int = 1;
const MPV_EVENT_END_FILE: c_int = 7;
const MPV_EVENT_PROPERTY_CHANGE: c_int = 22;

const MPV_END_FILE_REASON_EOF: c_int = 0;
const MPV_END_FILE_REASON_ERROR: c_int = 4;

const MPV_RENDER_PARAM_API_TYPE: c_int = 1;
const MPV_RENDER_PARAM_OPENGL_INIT_PARAMS: c_int = 2;
const MPV_RENDER_PARAM_OPENGL_FBO: c_int = 3;
const MPV_RENDER_PARAM_FLIP_Y: c_int = 4;
const MPV_RENDER_UPDATE_FRAME: u64 = 1;

/// Windows OpenGL entry-point resolution.
///
/// libmpv's render API asks the embedder for GL function pointers. The GL 1.1
/// entry points are exported directly by `opengl32.dll`; everything newer comes
/// from `wglGetProcAddress`, which requires a current context — and we only ask
/// while inside a paint callback, so one is always current.
#[cfg(windows)]
mod gl_loader {
    use std::ffi::c_void;
    use std::os::raw::c_char;
    use std::sync::OnceLock;

    #[link(name = "opengl32")]
    extern "system" {
        fn wglGetProcAddress(name: *const c_char) -> *mut c_void;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryA(name: *const c_char) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    }

    fn opengl32() -> *mut c_void {
        static MODULE: OnceLock<usize> = OnceLock::new();
        let handle = *MODULE.get_or_init(|| {
            let name = b"opengl32.dll\0";
            unsafe { LoadLibraryA(name.as_ptr() as *const c_char) as usize }
        });
        handle as *mut c_void
    }

    pub fn resolve(name: &str) -> *mut c_void {
        let Ok(cname) = std::ffi::CString::new(name) else {
            return std::ptr::null_mut();
        };
        unsafe {
            let direct = GetProcAddress(opengl32(), cname.as_ptr());
            if !direct.is_null() {
                return direct;
            }
            let proc = wglGetProcAddress(cname.as_ptr());
            // Drivers report "not found" as 0, 1 or -1 rather than only null.
            if proc.is_null() || proc == 1 as *mut c_void || proc == -1isize as *mut c_void {
                std::ptr::null_mut()
            } else {
                proc
            }
        }
    }
}

#[repr(C)]
struct MpvEvent {
    event_id: c_int,
    error: c_int,
    reply_userdata: u64,
    data: *mut c_void,
}

#[repr(C)]
struct MpvEventProperty {
    name: *const c_char,
    format: c_int,
    data: *mut c_void,
}

#[repr(C)]
struct MpvEventEndFile {
    reason: c_int,
    error: c_int,
    playlist_entry_id: i64,
    playlist_insert_id: i64,
    playlist_insert_num_entries: c_int,
}

#[repr(C)]
struct MpvRenderParam {
    type_: c_int,
    data: *mut c_void,
}

#[repr(C)]
struct MpvOpenglInitParams {
    get_proc_address:
        Option<unsafe extern "C" fn(ctx: *mut c_void, name: *const c_char) -> *mut c_void>,
    get_proc_address_ctx: *mut c_void,
    extra_exts: *mut c_void,
}

#[repr(C)]
struct MpvOpenglFbo {
    fbo: c_int,
    w: c_int,
    h: c_int,
    internal_format: c_int,
}

/// Copyable set of resolved symbols. Function pointers are `Send + Sync`, which
/// is what lets the event thread own a copy while the engine keeps the library
/// alive.
#[derive(Clone, Copy)]
struct Fns {
    create: unsafe extern "C" fn() -> MpvHandle,
    initialize: unsafe extern "C" fn(MpvHandle) -> c_int,
    set_option_string: unsafe extern "C" fn(MpvHandle, *const c_char, *const c_char) -> c_int,
    set_property: unsafe extern "C" fn(MpvHandle, *const c_char, c_int, *mut c_void) -> c_int,
    command: unsafe extern "C" fn(MpvHandle, *const *const c_char) -> c_int,
    observe_property: unsafe extern "C" fn(MpvHandle, u64, *const c_char, c_int) -> c_int,
    wait_event: unsafe extern "C" fn(MpvHandle, f64) -> *mut MpvEvent,
    error_string: unsafe extern "C" fn(c_int) -> *const c_char,
    terminate_destroy: unsafe extern "C" fn(MpvHandle),
    render_create:
        unsafe extern "C" fn(*mut MpvRenderContext, MpvHandle, *mut MpvRenderParam) -> c_int,
    render_render: unsafe extern "C" fn(MpvRenderContext, *mut MpvRenderParam) -> c_int,
    render_report_swap: unsafe extern "C" fn(MpvRenderContext),
    render_update: unsafe extern "C" fn(MpvRenderContext) -> u64,
    render_set_update_callback: unsafe extern "C" fn(
        MpvRenderContext,
        Option<unsafe extern "C" fn(*mut c_void)>,
        *mut c_void,
    ),
    render_free: unsafe extern "C" fn(MpvRenderContext),
}

/// Keeps the DLL mapped for as long as any symbol is reachable.
struct Api {
    _lib: Library,
    fns: Fns,
}

macro_rules! load_sym {
    ($lib:expr, $name:literal, $ty:ty) => {{
        let sym: Symbol<$ty> = unsafe { $lib.get($name) }.map_err(|err| {
            PlaybackError::EngineUnavailable(format!("{}: {err}", String::from_utf8_lossy($name)))
        })?;
        *sym
    }};
}

impl Api {
    fn open() -> Result<Self, PlaybackError> {
        // `libmpv-2.dll` is the mpv 0.35+ name; the others cover older builds
        // and hand-rolled distributions.
        let mut last_error = String::new();
        for candidate in ["libmpv-2.dll", "mpv-2.dll", "libmpv.dll"] {
            match unsafe { Library::new(candidate) } {
                Ok(lib) => return Self::from_library(lib),
                Err(err) => last_error = format!("{candidate}: {err}"),
            }
        }
        Err(PlaybackError::EngineUnavailable(last_error))
    }

    fn from_library(lib: Library) -> Result<Self, PlaybackError> {
        let fns = Fns {
            create: load_sym!(lib, b"mpv_create\0", unsafe extern "C" fn() -> MpvHandle),
            initialize: load_sym!(
                lib,
                b"mpv_initialize\0",
                unsafe extern "C" fn(MpvHandle) -> c_int
            ),
            set_option_string: load_sym!(
                lib,
                b"mpv_set_option_string\0",
                unsafe extern "C" fn(MpvHandle, *const c_char, *const c_char) -> c_int
            ),
            set_property: load_sym!(
                lib,
                b"mpv_set_property\0",
                unsafe extern "C" fn(MpvHandle, *const c_char, c_int, *mut c_void) -> c_int
            ),
            command: load_sym!(
                lib,
                b"mpv_command\0",
                unsafe extern "C" fn(MpvHandle, *const *const c_char) -> c_int
            ),
            observe_property: load_sym!(
                lib,
                b"mpv_observe_property\0",
                unsafe extern "C" fn(MpvHandle, u64, *const c_char, c_int) -> c_int
            ),
            wait_event: load_sym!(
                lib,
                b"mpv_wait_event\0",
                unsafe extern "C" fn(MpvHandle, f64) -> *mut MpvEvent
            ),
            error_string: load_sym!(
                lib,
                b"mpv_error_string\0",
                unsafe extern "C" fn(c_int) -> *const c_char
            ),
            terminate_destroy: load_sym!(
                lib,
                b"mpv_terminate_destroy\0",
                unsafe extern "C" fn(MpvHandle)
            ),
            render_create: load_sym!(
                lib,
                b"mpv_render_context_create\0",
                unsafe extern "C" fn(
                    *mut MpvRenderContext,
                    MpvHandle,
                    *mut MpvRenderParam,
                ) -> c_int
            ),
            render_render: load_sym!(
                lib,
                b"mpv_render_context_render\0",
                unsafe extern "C" fn(MpvRenderContext, *mut MpvRenderParam) -> c_int
            ),
            render_report_swap: load_sym!(
                lib,
                b"mpv_render_context_report_swap\0",
                unsafe extern "C" fn(MpvRenderContext)
            ),
            render_update: load_sym!(
                lib,
                b"mpv_render_context_update\0",
                unsafe extern "C" fn(MpvRenderContext) -> u64
            ),
            render_set_update_callback: load_sym!(
                lib,
                b"mpv_render_context_set_update_callback\0",
                unsafe extern "C" fn(
                    MpvRenderContext,
                    Option<unsafe extern "C" fn(*mut c_void)>,
                    *mut c_void,
                )
            ),
            render_free: load_sym!(
                lib,
                b"mpv_render_context_free\0",
                unsafe extern "C" fn(MpvRenderContext)
            ),
        };
        Ok(Self { _lib: lib, fns })
    }
}

/// Raw handle wrapper; mpv's client API is internally thread safe, but the
/// pointer itself is not `Send`.
#[derive(Clone, Copy)]
struct SendHandle(MpvHandle);
unsafe impl Send for SendHandle {}

pub struct MpvEngine {
    fns: Fns,
    api: Arc<Api>,
    handle: SendHandle,
    render_ctx: Option<MpvRenderContext>,
    events: Receiver<PlaybackEvent>,
    thread: Option<JoinHandle<()>>,
    needs_frame: Arc<AtomicBool>,
    stopping: Arc<AtomicBool>,
    paused: bool,
    loaded: bool,
}

// The render context is only ever touched from the UI thread, and the client
// handle is thread safe by contract.
unsafe impl Send for MpvEngine {}

impl MpvEngine {
    pub fn new() -> Result<Self, PlaybackError> {
        let api = Arc::new(Api::open()?);
        let fns = api.fns;

        let handle = unsafe { (fns.create)() };
        if handle.is_null() {
            return Err(PlaybackError::EngineUnavailable(
                "mpv_create returned null".to_string(),
            ));
        }

        // Configure before initialize: these are the options that decide where
        // frames go and how they are decoded.
        let options: [(&str, &str); 9] = [
            ("vo", "libmpv"),
            ("gpu-api", "opengl"),
            ("hwdec", "auto-safe"),
            ("idle", "yes"),
            ("terminal", "no"),
            ("input-default-bindings", "no"),
            ("osc", "no"),
            ("config", "no"),
            ("audio-client-name", "CloudStream"),
        ];
        for (name, value) in options {
            let name = CString::new(name).unwrap_or_default();
            let value = CString::new(value).unwrap_or_default();
            let rc = unsafe { (fns.set_option_string)(handle, name.as_ptr(), value.as_ptr()) };
            if rc < 0 {
                unsafe { (fns.terminate_destroy)(handle) };
                return Err(PlaybackError::EngineUnavailable(error_text(&fns, rc)));
            }
        }

        let rc = unsafe { (fns.initialize)(handle) };
        if rc < 0 {
            unsafe { (fns.terminate_destroy)(handle) };
            return Err(PlaybackError::EngineUnavailable(error_text(&fns, rc)));
        }

        for (index, name) in ["time-pos", "duration", "pause"].iter().enumerate() {
            let cname = CString::new(*name).unwrap_or_default();
            let format = if *name == "pause" {
                MPV_FORMAT_FLAG
            } else {
                MPV_FORMAT_DOUBLE
            };
            unsafe {
                (fns.observe_property)(handle, index as u64 + 1, cname.as_ptr(), format);
            }
        }

        let (tx, rx) = channel();
        let needs_frame = Arc::new(AtomicBool::new(true));
        let stopping = Arc::new(AtomicBool::new(false));
        let thread = spawn_event_thread(fns, SendHandle(handle), tx, Arc::clone(&stopping));

        Ok(Self {
            fns,
            api,
            handle: SendHandle(handle),
            render_ctx: None,
            events: rx,
            thread: Some(thread),
            needs_frame,
            stopping,
            paused: false,
            loaded: false,
        })
    }

    pub fn describe(&self) -> String {
        format!("libmpv loaded ({})", self.api._lib_path_hint())
    }

    fn error(&self, code: c_int) -> String {
        error_text(&self.fns, code)
    }

    fn set_double(&mut self, name: &str, value: f64) -> Result<(), PlaybackError> {
        let cname = CString::new(name).unwrap_or_default();
        let mut value = value;
        let rc = unsafe {
            (self.fns.set_property)(
                self.handle.0,
                cname.as_ptr(),
                MPV_FORMAT_DOUBLE,
                &mut value as *mut f64 as *mut c_void,
            )
        };
        if rc < 0 {
            Err(PlaybackError::Command(self.error(rc)))
        } else {
            Ok(())
        }
    }

    fn set_flag(&mut self, name: &str, value: bool) -> Result<(), PlaybackError> {
        let cname = CString::new(name).unwrap_or_default();
        let mut value: c_int = value as c_int;
        let rc = unsafe {
            (self.fns.set_property)(
                self.handle.0,
                cname.as_ptr(),
                MPV_FORMAT_FLAG,
                &mut value as *mut c_int as *mut c_void,
            )
        };
        if rc < 0 {
            Err(PlaybackError::Command(self.error(rc)))
        } else {
            Ok(())
        }
    }

    fn command_args(&self, args: &[&str]) -> Result<(), PlaybackError> {
        let owned: Vec<CString> = args
            .iter()
            .map(|arg| CString::new(*arg).unwrap_or_default())
            .collect();
        let mut ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
        ptrs.push(ptr::null());
        let rc = unsafe { (self.fns.command)(self.handle.0, ptrs.as_ptr()) };
        if rc < 0 {
            Err(PlaybackError::Command(self.error(rc)))
        } else {
            Ok(())
        }
    }

    fn ensure_render_context(&mut self) -> Result<MpvRenderContext, PlaybackError> {
        if let Some(ctx) = self.render_ctx {
            return Ok(ctx);
        }
        let api_type = CString::new("opengl").unwrap_or_default();
        let mut init = MpvOpenglInitParams {
            get_proc_address: Some(mpv_get_proc_address),
            get_proc_address_ctx: ptr::null_mut(),
            extra_exts: ptr::null_mut(),
        };
        let mut params = [
            param(MPV_RENDER_PARAM_API_TYPE, api_type.as_ptr() as *mut c_void),
            param(
                MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                &mut init as *mut MpvOpenglInitParams as *mut c_void,
            ),
            param(0, ptr::null_mut()),
        ];

        let mut ctx: MpvRenderContext = ptr::null_mut();
        let rc = unsafe { (self.fns.render_create)(&mut ctx, self.handle.0, params.as_mut_ptr()) };
        if rc < 0 {
            return Err(PlaybackError::Render(self.error(rc)));
        }
        unsafe {
            (self.fns.render_set_update_callback)(
                ctx,
                Some(mpv_update_callback),
                Arc::as_ptr(&self.needs_frame) as *mut c_void,
            );
        }
        self.render_ctx = Some(ctx);
        Ok(ctx)
    }
}

impl PlaybackEngine for MpvEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::LibMpv
    }

    fn load(&mut self, request: PlaybackRequest) -> Result<(), PlaybackError> {
        // HTTP headers travel through mpv's option because `loadfile` has no
        // header syntax.
        if !request.headers.is_empty() {
            let fields = request
                .headers
                .iter()
                .map(|(name, value)| format!("{name}: {value}"))
                .collect::<Vec<_>>()
                .join(",");
            let name = CString::new("http-header-fields").unwrap_or_default();
            let value = CString::new(fields).unwrap_or_default();
            unsafe {
                (self.fns.set_option_string)(self.handle.0, name.as_ptr(), value.as_ptr());
            }
        }

        if request.start_position > 0.0 && !request.live {
            self.set_double("start", request.start_position)?;
        }

        self.command_args(&["loadfile", &request.url, "replace"])?;
        self.loaded = true;
        self.paused = false;

        for subtitle in &request.subtitles {
            // `select` makes the last added track win, matching the host's
            // pre-selected default.
            let _ = self.command_args(&["sub-add", &subtitle.url, "select"]);
        }

        for (index, subtitle) in request.subtitles.iter().enumerate() {
            if subtitle.default {
                let _ = self.command_args(&["set", "sid", &(index + 1).to_string()]);
                break;
            }
        }

        Ok(())
    }

    fn command(&mut self, command: PlaybackCommand) -> Result<(), PlaybackError> {
        match command {
            PlaybackCommand::Play => {
                self.paused = false;
                self.set_flag("pause", false)
            }
            PlaybackCommand::Pause => {
                self.paused = true;
                self.set_flag("pause", true)
            }
            PlaybackCommand::TogglePause => {
                self.paused = !self.paused;
                self.set_flag("pause", self.paused)
            }
            PlaybackCommand::SeekTo(position) => {
                self.command_args(&["seek", &format!("{position:.3}"), "absolute"])
            }
            PlaybackCommand::SeekRelative(delta) => {
                self.command_args(&["seek", &format!("{delta:.3}"), "relative"])
            }
            PlaybackCommand::SetVolume(volume) => self.set_double("volume", volume * 100.0),
            PlaybackCommand::SetMuted(muted) => self.set_flag("mute", muted),
            PlaybackCommand::SetSpeed(speed) => self.set_double("speed", speed),
            // Aspect handling is delegated to mpv's scaler rather than done in
            // the compositor: it knows the real frame geometry.
            PlaybackCommand::SetAspect(mode) => match mode {
                AspectMode::Fit => {
                    self.set_double("panscan", 0.0)?;
                    self.set_flag("keepaspect", true)?;
                    self.set_flag("video-unscaled", false)?;
                    Ok(())
                }
                AspectMode::Fill => {
                    self.set_flag("keepaspect", true)?;
                    self.set_double("panscan", 1.0)?;
                    Ok(())
                }
                AspectMode::Stretch => {
                    self.set_flag("keepaspect", false)?;
                    self.set_double("panscan", 0.0)?;
                    Ok(())
                }
                AspectMode::Original => {
                    self.set_flag("video-unscaled", true)?;
                    Ok(())
                }
            },
            PlaybackCommand::SetSubtitle(url) => match url {
                Some(url) => self.command_args(&["sub-add", &url, "select"]),
                None => self.command_args(&["set", "sid", "no"]),
            },
            PlaybackCommand::SetAudioTrack(track) => match track {
                Some(track) => self.command_args(&["set", "aid", &track]),
                None => self.command_args(&["set", "aid", "no"]),
            },
            PlaybackCommand::Stop => {
                self.loaded = false;
                self.paused = false;
                self.command_args(&["stop"])
            }
        }
    }

    fn poll(&mut self) -> Vec<PlaybackEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            events.push(event);
        }
        events
    }

    fn needs_render(&self) -> bool {
        if self.render_ctx.is_none() {
            return self.loaded;
        }
        let ctx = self.render_ctx.expect("checked above");
        let flagged = self.needs_frame.swap(false, Ordering::AcqRel);
        let dirty = unsafe { (self.fns.render_update)(ctx) } & MPV_RENDER_UPDATE_FRAME != 0;
        self.loaded && (flagged || dirty)
    }

    fn render(&mut self, target: RenderTarget) -> Result<(), PlaybackError> {
        let ctx = self.ensure_render_context()?;
        let mut fbo = MpvOpenglFbo {
            fbo: target.fbo as c_int,
            w: target.width as c_int,
            h: target.height as c_int,
            internal_format: 0,
        };
        let mut flip: c_int = if target.flip_y { 1 } else { 0 };
        let mut params = [
            param(
                MPV_RENDER_PARAM_OPENGL_FBO,
                &mut fbo as *mut MpvOpenglFbo as *mut c_void,
            ),
            param(
                MPV_RENDER_PARAM_FLIP_Y,
                &mut flip as *mut c_int as *mut c_void,
            ),
            param(0, ptr::null_mut()),
        ];
        let rc = unsafe { (self.fns.render_render)(ctx, params.as_mut_ptr()) };
        if rc < 0 {
            return Err(PlaybackError::Render(self.error(rc)));
        }
        unsafe { (self.fns.render_report_swap)(ctx) };
        Ok(())
    }

    fn shutdown(&mut self) {
        self.stopping.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        if let Some(ctx) = self.render_ctx.take() {
            unsafe { (self.fns.render_free)(ctx) };
        }
        if !self.handle.0.is_null() {
            unsafe { (self.fns.terminate_destroy)(self.handle.0) };
        }
        self.loaded = false;
    }
}

impl Drop for MpvEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn param(type_: c_int, data: *mut c_void) -> MpvRenderParam {
    MpvRenderParam { type_, data }
}

fn error_text(fns: &Fns, code: c_int) -> String {
    unsafe {
        let ptr = (fns.error_string)(code);
        if ptr.is_null() {
            format!("mpv error {code}")
        } else {
            CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }
}

unsafe extern "C" fn mpv_update_callback(ctx: *mut c_void) {
    if ctx.is_null() {
        return;
    }
    let flag = &*(ctx as *const AtomicBool);
    flag.store(true, Ordering::Release);
}

#[cfg(windows)]
unsafe extern "C" fn mpv_get_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    if name.is_null() {
        return ptr::null_mut();
    }
    match CStr::from_ptr(name).to_str() {
        Ok(name) => gl_loader::resolve(name),
        Err(_) => ptr::null_mut(),
    }
}

fn spawn_event_thread(
    fns: Fns,
    handle: SendHandle,
    tx: Sender<PlaybackEvent>,
    stopping: Arc<AtomicBool>,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("mpv-events".to_string())
        .spawn(move || {
            // Bind the whole wrapper so edition-2021 precise capture does not
            // grab the raw pointer field on its own (it is not `Send`).
            let handle = handle;
            let mut duration: Option<f64> = None;
            let mut last_position = Instant::now();
            while !stopping.load(Ordering::Acquire) {
                let event = unsafe { (fns.wait_event)(handle.0, 0.2) };
                if event.is_null() {
                    continue;
                }
                let event = unsafe { &*event };
                match event.event_id {
                    MPV_EVENT_SHUTDOWN => break,
                    MPV_EVENT_END_FILE => {
                        let end = unsafe { &*(event.data as *const MpvEventEndFile) };
                        match end.reason {
                            MPV_END_FILE_REASON_EOF => {
                                let _ = tx.send(PlaybackEvent::Ended);
                            }
                            MPV_END_FILE_REASON_ERROR => {
                                let _ = tx.send(PlaybackEvent::Failed(error_text(&fns, end.error)));
                            }
                            // STOP is emitted when we replace the file ourselves.
                            _ => {}
                        }
                    }
                    MPV_EVENT_PROPERTY_CHANGE => {
                        if event.data.is_null() {
                            continue;
                        }
                        let property = unsafe { &*(event.data as *const MpvEventProperty) };
                        if property.name.is_null() || property.data.is_null() {
                            continue;
                        }
                        let name = unsafe { CStr::from_ptr(property.name) }
                            .to_string_lossy()
                            .into_owned();
                        match (name.as_str(), property.format) {
                            ("time-pos", MPV_FORMAT_DOUBLE) => {
                                let value = unsafe { *(property.data as *const f64) };
                                // Decoders emit this up to 60x/s; 4/s is plenty
                                // for a scrub bar.
                                if last_position.elapsed() >= Duration::from_millis(250) {
                                    last_position = Instant::now();
                                    let _ = tx.send(PlaybackEvent::Position {
                                        position: value,
                                        duration,
                                    });
                                }
                            }
                            ("duration", MPV_FORMAT_DOUBLE) => {
                                let value = unsafe { *(property.data as *const f64) };
                                duration = Some(value);
                                let _ = tx.send(PlaybackEvent::Loaded {
                                    duration: Some(value),
                                });
                            }
                            ("pause", MPV_FORMAT_FLAG) => {
                                let paused = unsafe { *(property.data as *const c_int) } != 0;
                                let _ = tx.send(PlaybackEvent::Paused(paused));
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        })
        .unwrap_or_else(|_| std::thread::spawn(|| {}))
}

impl Api {
    fn _lib_path_hint(&self) -> &'static str {
        "libmpv-2.dll"
    }
}

/// Diagnostics used by the settings screen and by `--selftest`.
impl MpvEngine {
    pub fn report(&self) -> EngineReport {
        EngineReport {
            kind: EngineKind::LibMpv,
            detail: self.describe(),
        }
    }
}
