//! CloudStream Desktop.
//!
//! This binary is the only place that wires everything together: it opens
//! storage, starts the workers, creates the playback engine once an OpenGL
//! context exists, and owns the frame loop:
//!
//! ```text
//! drain results ─▶ reduce ─▶ apply effects        (background work)
//!            └───▶ draw ─▶ reduce UI intents ─▶ apply effects
//! ```
//!
//! `--selftest` runs the same subsystems without a window, which is what CI and
//! a fresh clone use to prove the install is sound.

// A release build is a window application: launching it from Explorer or from
// the installer must not open a console. Debug builds keep one, because that is
// where their diagnostics go; `cli_out` reattaches to a terminal in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod probe;
mod runtime;
mod selftest;

use app_core::{Effect, Intent, PlaybackCommand, ToastKind};
use app_playback::create_engine;
use app_ui::Ui;
use runtime::Runtime;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|arg| arg == "--selftest") {
        let (code, report) = selftest::run();
        cli_out(&report);
        std::process::exit(code);
    }
    if let Some(index) = args.iter().position(|arg| arg == "--probe") {
        let id = args
            .get(index + 1)
            .map(String::as_str)
            .unwrap_or("builtin-ctgmovies");
        let (code, report) = probe::run(id);
        cli_out(&report);
        std::process::exit(code);
    }
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        cli_out(&format!(
            "CloudStream Desktop {}\n",
            env!("CARGO_PKG_VERSION")
        ));
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        cli_out(&format!(
            "CloudStream Desktop {}\n\n\
             A media centre with no bundled content: install extensions or use the\n\
             built-in providers to add sources.\n\n\
             Usage: cloudstream-desktop [OPTIONS]\n\n\
             Options:\n  \
               --selftest       Exercise storage, reducer, layout and engines, then exit\n  \
               --probe <id>     Run one built-in provider against its live site\n  \
               --version        Print the version\n  \
               --help           Print this help\n\n\
             Playback needs libmpv-2.dll beside the executable; without it the player\n\
             runs as a timeline and Settings → Diagnostics says so.",
            env!("CARGO_PKG_VERSION")
        ));
        return Ok(());
    }

    let runtime = match Runtime::boot() {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("startup failed: {err}");
            std::process::exit(1);
        }
    };

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            // Small enough for a snapped half-screen window; the layout has a
            // Compact breakpoint below 720 px.
            .with_min_inner_size([360.0, 420.0])
            .with_title("CloudStream Desktop"),
        renderer: eframe::Renderer::Glow,
        vsync: true,
        ..Default::default()
    };

    eframe::run_native(
        "CloudStream Desktop",
        options,
        Box::new(move |cc| Ok(Box::new(DesktopApp::new(cc, runtime)))),
    )
}

struct DesktopApp {
    ui: Ui,
    runtime: Runtime,
}

impl DesktopApp {
    fn new(_cc: &eframe::CreationContext<'_>, mut runtime: Runtime) -> Self {
        // The decoder resolves its own GL entry points (WGL) when it first renders,
        // inside a paint callback where a context is current, so it needs nothing
        // from the creation context.
        let (engine, report) = create_engine();
        runtime.engine = Some(Arc::new(Mutex::new(engine)));

        if let Ok(mut state) = runtime.state.lock() {
            let _ = app_core::reduce(
                &mut state,
                Intent::PlayerReady {
                    engine: report.kind,
                    detail: report.detail,
                },
            );
        }

        // Build the provider registry from whatever is installed on disk.
        runtime.reload_providers();

        Self {
            ui: Ui::new(),
            runtime,
        }
    }

    /// Execute one effect. Everything that can block was already routed to the
    /// work thread; this handles the player and the small local files only.
    fn apply(&mut self, effect: Effect) {
        match effect {
            Effect::PlayerLoad { request } => {
                let outcome = self
                    .runtime
                    .engine
                    .as_ref()
                    .map(|engine| engine.lock().map(|mut engine| engine.load(*request)));
                match outcome {
                    Some(Ok(Ok(()))) => {}
                    Some(Ok(Err(err))) => {
                        self.notice(ToastKind::Error, format!("Cannot play: {err}"));
                    }
                    Some(Err(_)) => self.notice(ToastKind::Error, "Player is unavailable."),
                    None => self.notice(ToastKind::Error, "No playback engine."),
                }
            }
            Effect::PlayerCommand(command) => self.runtime.send_player_command(command),
            Effect::PlayerStop => self.runtime.send_player_command(PlaybackCommand::Stop),
            Effect::PersistSettings => {
                if let Ok(state) = self.runtime.state.lock() {
                    self.runtime.save_settings(&state.settings);
                }
            }
            Effect::PersistBookmarks => {
                if let Ok(state) = self.runtime.state.lock() {
                    self.runtime.save_bookmarks(&state);
                }
            }
            Effect::PersistProgress => {
                if let Ok(state) = self.runtime.state.lock() {
                    self.runtime.save_progress(&state);
                }
            }
            Effect::PersistRepositories => {
                if let Ok(state) = self.runtime.state.lock() {
                    self.runtime.save_repositories(&state);
                }
            }
            Effect::PersistExtensions => {
                if let Ok(state) = self.runtime.state.lock() {
                    self.runtime.save_extensions(&state);
                }
            }
            other => self.runtime.send(other),
        }
    }

    fn notice(&self, kind: ToastKind, message: impl Into<String>) {
        if let Ok(mut state) = self.runtime.state.lock() {
            let _ = app_core::reduce(
                &mut state,
                Intent::Notice {
                    kind,
                    message: message.into(),
                },
            );
        }
    }

    fn reduce_all(&mut self, intents: Vec<Intent>) {
        let mut effects = Vec::new();
        if let Ok(mut state) = self.runtime.state.lock() {
            for intent in intents {
                effects.extend(app_core::reduce(&mut state, intent));
            }
        }
        for effect in effects {
            self.apply(effect);
        }
    }
}

impl eframe::App for DesktopApp {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        // 1. background results + decoder events
        let mut intents = self.runtime.drain_results();
        if let Some(engine) = &self.runtime.engine {
            if let Ok(mut engine) = engine.lock() {
                for event in engine.poll() {
                    intents.push(Intent::PlayerEvent(event));
                }
            }
        }
        if let Ok(mut state) = self.runtime.state.lock() {
            state.expire_toasts(Duration::from_secs(6));
        }
        self.reduce_all(intents);

        // 2. draw
        let ui_intents = match self.runtime.state.lock() {
            Ok(state) => self.ui.show(ctx, &state, self.runtime.engine.clone()),
            Err(_) => return,
        };

        // 3. react
        self.reduce_all(ui_intents);

        // 4. repaint policy: only spin when something is actually moving.
        let busy = self
            .runtime
            .state
            .lock()
            .map(|state| {
                state.player.visible
                    || state.library.loading
                    || state.search.is_pending()
                    || state.extensions.busy
                    || state
                        .details
                        .as_ref()
                        .is_some_and(|d| d.loading || d.resolving)
            })
            .unwrap_or(false);
        if busy {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    fn clear_color(&self, _visuals: &eframe::egui::Visuals) -> [f32; 4] {
        // Matches the dark palette so the first frame does not flash white.
        [0.059, 0.067, 0.082, 1.0]
    }
}

// ---------------------------------------------------------------------------
// Console output
// ---------------------------------------------------------------------------

/// Print to the terminal that launched us, if there is one.
fn cli_out(text: &str) {
    #[cfg(all(windows, not(debug_assertions)))]
    {
        // A window-subsystem process still inherits a pipe or a redirect when a
        // shell started it; only when there is none (Explorer, a shortcut) do we
        // have to reach for the parent console ourselves.
        if !console::stdout_is_usable() {
            if let Some(mut console) = console::attach() {
                use std::io::Write;
                let _ = console.write_all(text.as_bytes());
                let _ = console.flush();
                return;
            }
        }
    }
    print!("{text}");
}

#[cfg(all(windows, not(debug_assertions)))]
mod console {
    use std::ffi::c_void;

    /// `ATTACH_PARENT_PROCESS` and `STD_OUTPUT_HANDLE` as the Win32 API wants them.
    const ATTACH_PARENT_PROCESS: u32 = 0xFFFF_FFFF;
    const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5;
    /// `GetFileType` returns this for an invalid or closed handle.
    const FILE_TYPE_UNKNOWN: u32 = 0;

    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
        fn GetStdHandle(which: u32) -> *mut c_void;
        fn GetFileType(handle: *mut c_void) -> u32;
    }

    /// Whether this process was given a real stdout by whoever launched it.
    pub fn stdout_is_usable() -> bool {
        unsafe {
            let handle = GetStdHandle(STD_OUTPUT_HANDLE);
            !handle.is_null() && GetFileType(handle) != FILE_TYPE_UNKNOWN
        }
    }

    /// A writable handle to the parent console, or `None` when the process was
    /// started from Explorer and therefore has no terminal.
    pub fn attach() -> Option<std::fs::File> {
        unsafe {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
        std::fs::OpenOptions::new().write(true).open("CONOUT$").ok()
    }
}
