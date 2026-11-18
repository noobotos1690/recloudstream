//! Adaptive user interface.
//!
//! The UI is a pure projection of [`AppState`]: it borrows state, renders it, and
//! returns a list of [`Intent`]s. It never mutates state, never performs I/O and
//! never talks to a provider or the decoder directly. That is what makes the
//! reducer testable and what lets the same state render at 320 px or 3840 px.
//!
//! Layout follows the three breakpoints computed in `app-core::layout`:
//!
//! | Width (logical px) | Navigation        | Grid            |
//! |--------------------|-------------------|-----------------|
//! | < 720              | bottom bar        | 1–3 columns     |
//! | 720–1200           | icon rail         | 3–5 columns     |
//! | > 1200             | labelled rail     | 5–8 columns     |

mod covers;
mod overlays;
mod screens;
mod theme;
mod video;
mod widgets;

pub use covers::CoverCache;
pub use theme::Palette;
pub use video::{EngineHandle, VideoSurface};

use app_core::{AppState, Intent, LayoutMetrics, PlaybackCommand, Route, ToastKind};
use egui::{Align2, Margin, RichText, Vec2};

pub struct Ui {
    theme: theme::ThemeCache,
    covers: CoverCache,
    video: VideoSurface,
}

impl Default for Ui {
    fn default() -> Self {
        Self::new()
    }
}

impl Ui {
    pub fn new() -> Self {
        Self {
            theme: theme::ThemeCache::new(),
            covers: CoverCache::new(),
            video: VideoSurface::new(),
        }
    }

    /// Render one frame and return everything the user asked for.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        state: &AppState,
        engine: Option<EngineHandle>,
    ) -> Vec<Intent> {
        let mut intents = Vec::new();

        let screen = ctx.screen_rect();
        // Report the window size every frame: it is the single input the layout
        // engine needs, and it also covers DPI changes when dragging between
        // monitors.
        intents.push(Intent::Resize {
            width: screen.width(),
            height: screen.height(),
            dpi: ctx.pixels_per_point(),
        });

        let palette = self.theme.apply(ctx, &state.settings);
        if !state.backend.data_dir.is_empty() {
            self.covers.set_dir(
                std::path::Path::new(&state.backend.data_dir)
                    .join("cache")
                    .join("covers"),
            );
        }

        if state.player.visible {
            self.player_keys(ctx, &mut intents);
            overlays::player(ctx, state, engine, palette, &mut self.video, &mut intents);
            self.toasts(ctx, state, palette, &mut intents);
            return intents;
        }

        let layout = state.layout();

        egui::TopBottomPanel::top("top_bar")
            .exact_height(50.0)
            .frame(
                egui::Frame::default()
                    .fill(palette.bg)
                    .inner_margin(Margin::symmetric(12, 8)),
            )
            .show(ctx, |ui| {
                self.top_bar(ui, state, &layout, palette, &mut intents);
            });

        match layout.nav {
            app_core::layout::NavMode::BottomBar => {
                egui::TopBottomPanel::bottom("bottom_nav")
                    .exact_height(layout.bottom_bar_height)
                    .frame(
                        egui::Frame::default()
                            .fill(palette.surface)
                            .inner_margin(Margin::symmetric(4, 4)),
                    )
                    .show(ctx, |ui| {
                        // Compute the item width before entering the horizontal
                        // layout, otherwise `available_width` shrinks per item.
                        let item_width = ui.available_width() / Route::ALL.len() as f32;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::ZERO;
                            for route in Route::ALL {
                                let response = widgets::nav_item(
                                    ui,
                                    route.label(),
                                    icon_for(route),
                                    state.route == route,
                                    false,
                                    Vec2::new(item_width, 44.0),
                                    palette,
                                );
                                if response.clicked() {
                                    intents.push(Intent::Navigate(route));
                                }
                            }
                        });
                    });
            }
            nav_mode => {
                let with_labels = matches!(nav_mode, app_core::layout::NavMode::RailWithLabels);
                egui::SidePanel::left("nav_rail")
                    .exact_width(layout.rail_width)
                    .resizable(false)
                    .frame(
                        egui::Frame::default()
                            .fill(palette.surface)
                            .inner_margin(Margin::symmetric(8, 10)),
                    )
                    .show(ctx, |ui| {
                        ui.add_space(2.0);
                        if with_labels {
                            ui.label(
                                RichText::new("CloudStream")
                                    .size(13.0)
                                    .strong()
                                    .color(palette.text),
                            );
                        }
                        ui.add_space(12.0);
                        for route in Route::ALL {
                            let size = if with_labels {
                                Vec2::new(ui.available_width(), 40.0)
                            } else {
                                Vec2::splat(44.0)
                            };
                            let response = widgets::nav_item(
                                ui,
                                route.label(),
                                icon_for(route),
                                state.route == route,
                                with_labels,
                                size,
                                palette,
                            );
                            if response.clicked() {
                                intents.push(Intent::Navigate(route));
                            }
                            ui.add_space(2.0);
                        }
                    });
            }
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::default()
                    .fill(palette.bg)
                    .inner_margin(Margin::symmetric(layout.content_padding as i8, 6)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        screens::render(ui, state, layout, palette, &mut self.covers, &mut intents);
                    });
            });

        if state.details.is_some() {
            overlays::details(ctx, state, &mut self.covers, palette, &mut intents);
        }
        self.toasts(ctx, state, palette, &mut intents);
        intents
    }

    fn top_bar(
        &mut self,
        ui: &mut egui::Ui,
        state: &AppState,
        layout: &LayoutMetrics,
        palette: Palette,
        intents: &mut Vec<Intent>,
    ) {
        ui.horizontal_centered(|ui| {
            let title = match state.route {
                Route::Home => "CloudStream",
                route => route.label(),
            };
            ui.label(RichText::new(title).size(14.0).strong().color(palette.text));
            ui.add_space(10.0);

            let mut query = state.search.query.clone();
            let field_width = if layout.breakpoint == app_core::layout::Breakpoint::Compact {
                (ui.available_width() - 60.0).max(120.0)
            } else {
                (ui.available_width() - 120.0).clamp(160.0, 420.0)
            };
            let response = ui.add(
                egui::TextEdit::singleline(&mut query)
                    .hint_text("Search installed providers")
                    .desired_width(field_width),
            );
            if query != state.search.query && !query.is_empty() {
                intents.push(Intent::SearchInput(query.clone()));
            }
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                intents.push(Intent::SearchSubmit(query));
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::icon_button(
                    ui,
                    widgets::Icon::Refresh,
                    None,
                    Vec2::splat(30.0),
                    false,
                    palette,
                    false,
                )
                .clicked()
                {
                    intents.push(Intent::RefreshHome);
                }
                if state.library.loading {
                    ui.spinner();
                }
            });
        });
    }

    fn toasts(
        &mut self,
        ctx: &egui::Context,
        state: &AppState,
        palette: Palette,
        intents: &mut Vec<Intent>,
    ) {
        if state.toasts.is_empty() {
            return;
        }
        egui::Area::new(egui::Id::new("toasts"))
            .order(egui::Order::Foreground)
            .anchor(Align2::RIGHT_BOTTOM, [-16.0, -16.0])
            .show(ctx, |ui| {
                for (index, toast) in state.toasts.iter().enumerate() {
                    let is_error = matches!(toast.kind, ToastKind::Error | ToastKind::Warning);
                    let response = widgets::toast(ui, &toast.message, palette, is_error);
                    if response.clicked() {
                        intents.push(Intent::DismissToast(index));
                    }
                    ui.add_space(4.0);
                }
            });
    }

    fn player_keys(&self, ctx: &egui::Context, intents: &mut Vec<Intent>) {
        // Read input first, then act: sending a viewport command from inside
        // `ctx.input` would re-enter the context lock.
        let (toggle, back, left, right, mute, fullscreen) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Space),
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::M),
                i.key_pressed(egui::Key::F),
            )
        });

        if toggle {
            intents.push(Intent::PlayerCommand(PlaybackCommand::TogglePause));
        }
        if back {
            intents.push(Intent::Back);
        }
        if left {
            intents.push(Intent::SeekRelative(-10.0));
        }
        if right {
            intents.push(Intent::SeekRelative(10.0));
        }
        if mute {
            intents.push(Intent::ToggleMute);
        }
        if fullscreen {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
        }
    }
}

fn icon_for(route: Route) -> widgets::Icon {
    match route {
        Route::Home => widgets::Icon::Home,
        Route::Browse => widgets::Icon::Browse,
        Route::Search => widgets::Icon::Search,
        Route::Bookmarks => widgets::Icon::Bookmark,
        Route::Extensions => widgets::Icon::Extensions,
        Route::Settings => widgets::Icon::Settings,
    }
}
