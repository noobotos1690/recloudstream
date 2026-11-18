//! Video surface and OpenGL compositor.
//!
//! The player does not open a child window for video. Instead the decoder renders
//! into an offscreen framebuffer owned by this module, and that framebuffer is
//! blitted into the window at the exact rect the layout gave to the video
//! surface, during egui's own paint pass.
//!
//! Why this shape:
//!
//! * one GL context, so resizing cannot desynchronise two windows;
//! * the blit happens inside the image's z-order, so overlays can sit on top;
//! * while paused the framebuffer keeps the last frame, so no re-decode is needed
//!   to redraw.
//!
//! When no decoder is available the surface paints a neutral panel instead, which
//! is why the app is still usable without `libmpv-2.dll`.

use app_core::{EngineKind, PlaybackEngine, RenderTarget};
use egui::{Color32, CornerRadius, Rect, Ui};
use std::sync::{Arc, Mutex};

/// Shared engine handle: the UI thread renders into it, the runtime thread sends
/// transport commands to it.
pub type EngineHandle = Arc<Mutex<Box<dyn PlaybackEngine>>>;

pub struct VideoSurface {
    compositor: Arc<Mutex<Compositor>>,
}

impl Default for VideoSurface {
    fn default() -> Self {
        Self {
            compositor: Arc::new(Mutex::new(Compositor::default())),
        }
    }
}

impl VideoSurface {
    pub fn new() -> Self {
        Self::default()
    }

    /// Paint the decoder output — or a placeholder — into `rect`.
    pub fn paint(
        &self,
        ui: &mut Ui,
        rect: Rect,
        engine: Option<EngineHandle>,
        kind: EngineKind,
        palette: crate::theme::Palette,
    ) {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(6), Color32::BLACK);

        let Some(engine) = engine else {
            self.placeholder(ui, rect, kind, palette);
            return;
        };

        let compositor = Arc::clone(&self.compositor);
        let callback = egui::PaintCallback {
            rect,
            callback: Arc::new(eframe::egui_glow::CallbackFn::new(move |info, painter| {
                let viewport = info.viewport_in_pixels();
                let width = viewport.width_px.max(1);
                let height = viewport.height_px.max(1);
                let mut compositor = match compositor.lock() {
                    Ok(guard) => guard,
                    Err(_) => return,
                };
                if let Err(err) = compositor.present(
                    painter.gl(),
                    width,
                    height,
                    viewport.left_px,
                    viewport.from_bottom_px,
                    &engine,
                ) {
                    // A compositing failure must not take the app down; the
                    // player shows its own error state from engine events.
                    eprintln!("video: {err}");
                }
            })),
        };
        ui.painter().add(callback);

        if kind != EngineKind::LibMpv {
            self.placeholder(ui, rect, kind, palette);
        }
    }

    fn placeholder(
        &self,
        ui: &mut Ui,
        rect: Rect,
        kind: EngineKind,
        palette: crate::theme::Palette,
    ) {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(6),
            Color32::from_rgb(0x06, 0x08, 0x0B),
        );
        let text = match kind {
            EngineKind::LibMpv => "Waiting for the first frame…",
            EngineKind::Null => "No decoder available — drop libmpv-2.dll beside the executable",
        };
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(13.0),
            palette.text_dim,
        );
    }
}

#[derive(Default)]
struct Compositor {
    framebuffer: Option<eframe::glow::Framebuffer>,
    texture: Option<eframe::glow::Texture>,
    size: (i32, i32),
}

impl Compositor {
    fn present(
        &mut self,
        gl: &eframe::glow::Context,
        width: i32,
        height: i32,
        left: i32,
        bottom: i32,
        engine: &EngineHandle,
    ) -> Result<(), String> {
        use eframe::glow::HasContext;

        self.ensure(gl, width, height)?;
        let framebuffer = self
            .framebuffer
            .ok_or_else(|| "framebuffer was not created".to_string())?;

        let needs_frame = engine
            .lock()
            .map(|engine| engine.needs_render())
            .unwrap_or(false);

        if needs_frame {
            let mut viewport = [0i32; 4];
            unsafe {
                gl.get_parameter_i32_slice(eframe::glow::VIEWPORT, &mut viewport);
                gl.bind_framebuffer(eframe::glow::FRAMEBUFFER, Some(framebuffer));
                gl.viewport(0, 0, width, height);
            }

            let target = RenderTarget {
                fbo: raw_framebuffer(framebuffer),
                width: width as u32,
                height: height as u32,
                // mpv renders bottom-up; the blit below keeps that orientation.
                flip_y: true,
            };
            {
                let mut guard = engine
                    .lock()
                    .map_err(|_| "playback engine is unavailable".to_string())?;
                guard.render(target).map_err(|err| err.to_string())?;
            }

            unsafe {
                gl.bind_framebuffer(eframe::glow::FRAMEBUFFER, None);
                gl.viewport(viewport[0], viewport[1], viewport[2], viewport[3]);
            }
        }

        unsafe {
            gl.bind_framebuffer(eframe::glow::READ_FRAMEBUFFER, Some(framebuffer));
            // eframe with the glow backend renders into the default framebuffer.
            gl.bind_framebuffer(eframe::glow::DRAW_FRAMEBUFFER, None);
            gl.blit_framebuffer(
                0,
                0,
                width,
                height,
                left,
                bottom,
                left + width,
                bottom + height,
                eframe::glow::COLOR_BUFFER_BIT,
                eframe::glow::LINEAR,
            );
            gl.bind_framebuffer(eframe::glow::FRAMEBUFFER, None);
        }
        Ok(())
    }

    fn ensure(
        &mut self,
        gl: &eframe::glow::Context,
        width: i32,
        height: i32,
    ) -> Result<(), String> {
        use eframe::glow::HasContext;

        if self.size == (width, height) && self.framebuffer.is_some() {
            return Ok(());
        }

        unsafe {
            if self.framebuffer.is_none() {
                let texture = gl.create_texture().map_err(|err| err.to_string())?;
                let framebuffer = gl.create_framebuffer().map_err(|err| err.to_string())?;
                self.texture = Some(texture);
                self.framebuffer = Some(framebuffer);
            }
            let texture = self.texture.expect("created above");
            let framebuffer = self.framebuffer.expect("created above");

            gl.bind_texture(eframe::glow::TEXTURE_2D, Some(texture));
            // Immutable storage: no pixel data to supply, and the driver picks a
            // format it can render to without a round-trip.
            gl.tex_storage_2d(
                eframe::glow::TEXTURE_2D,
                1,
                eframe::glow::RGBA8,
                width,
                height,
            );
            gl.tex_parameter_i32(
                eframe::glow::TEXTURE_2D,
                eframe::glow::TEXTURE_MIN_FILTER,
                eframe::glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                eframe::glow::TEXTURE_2D,
                eframe::glow::TEXTURE_MAG_FILTER,
                eframe::glow::LINEAR as i32,
            );
            gl.bind_framebuffer(eframe::glow::FRAMEBUFFER, Some(framebuffer));
            gl.framebuffer_texture_2d(
                eframe::glow::FRAMEBUFFER,
                eframe::glow::COLOR_ATTACHMENT0,
                eframe::glow::TEXTURE_2D,
                Some(texture),
                0,
            );
            gl.bind_framebuffer(eframe::glow::FRAMEBUFFER, None);
            gl.bind_texture(eframe::glow::TEXTURE_2D, None);
        }
        self.size = (width, height);
        Ok(())
    }
}

/// Numeric GL name behind a glow framebuffer handle.
fn raw_framebuffer(framebuffer: eframe::glow::Framebuffer) -> u32 {
    framebuffer.0.get()
}
