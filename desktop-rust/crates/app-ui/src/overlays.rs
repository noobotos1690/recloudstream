//! Overlays: the details sheet and the player.
//!
//! Both are `Area`s on the foreground layer rather than windows, because a
//! window's title bar, drag handle and close button would fight the design — and
//! because the player must be able to cover the whole surface.

use crate::covers::CoverCache;
use crate::theme::Palette;
use crate::video::{EngineHandle, VideoSurface};
use crate::widgets::{self, Icon};
use app_core::{AppState, AspectMode, Intent, PlaybackCommand, PlayerPhase};
use egui::{Align2, Color32, CornerRadius, Id, Margin, Order, RichText, Sense, Ui, Vec2};

/// Dimmed backdrop drawn behind a centred overlay.
fn scrim(ctx: &egui::Context, id: &str) {
    let screen = ctx.screen_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(Order::Foreground, Id::new(id)));
    painter.rect_filled(
        screen,
        CornerRadius::ZERO,
        Color32::from_black_alpha(if ctx.style().visuals.dark_mode {
            200
        } else {
            120
        }),
    );
}

// ---------------------------------------------------------------------------
// Details
// ---------------------------------------------------------------------------

pub fn details(
    ctx: &egui::Context,
    state: &AppState,
    covers: &mut CoverCache,
    palette: Palette,
    intents: &mut Vec<Intent>,
) {
    let Some(details) = state.details.as_ref() else {
        return;
    };
    scrim(ctx, "details_scrim");

    let max_width = (ctx.screen_rect().width() - 64.0).clamp(320.0, 760.0);
    let max_height = ctx.screen_rect().height() - 64.0;

    egui::Area::new(Id::new("details_overlay"))
        .order(Order::Foreground)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(palette.surface)
                .corner_radius(CornerRadius::same(14))
                .inner_margin(Margin::same(18))
                .show(ui, |ui| {
                    ui.set_max_width(max_width);
                    egui::ScrollArea::vertical()
                        .max_height(max_height)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            hero(ui, state, details, covers, palette, intents);
                            ui.add_space(12.0);
                            sources(ui, state, details, palette, intents);
                            if let Some(loaded) = &details.details {
                                if !loaded.episodes.is_empty() {
                                    ui.add_space(12.0);
                                    episodes(ui, details, &loaded.episodes, palette, intents);
                                }
                            }
                        });
                });
        });
}

fn hero(
    ui: &mut Ui,
    state: &AppState,
    details: &app_core::state::DetailsState,
    covers: &mut CoverCache,
    palette: Palette,
    intents: &mut Vec<Intent>,
) {
    ui.horizontal(|ui| {
        let poster_size = Vec2::new(132.0, 194.0);
        let (rect, response) = ui.allocate_exact_size(poster_size, Sense::hover());
        match details
            .item
            .poster
            .as_ref()
            .and_then(|url| covers.get(ui.ctx(), url))
        {
            Some(texture) => {
                ui.painter().image(
                    texture.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            None => {
                let (top, bottom) = crate::theme::placeholder_tint(&details.item.key());
                let half = rect.height() * 0.55;
                ui.painter().rect_filled(
                    egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.min.y + half)),
                    CornerRadius::same(8),
                    top,
                );
                ui.painter().rect_filled(
                    egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.min.y + half), rect.max),
                    CornerRadius::same(8),
                    bottom,
                );
            }
        }
        let _ = response;

        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&details.item.title)
                        .size(20.0)
                        .strong()
                        .color(palette.text),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::icon_button(
                        ui,
                        Icon::Close,
                        None,
                        Vec2::splat(30.0),
                        false,
                        palette,
                        false,
                    )
                    .clicked()
                    {
                        intents.push(Intent::CloseDetails);
                    }
                });
            });

            let mut meta = vec![details.item.kind.label().to_string()];
            if let Some(year) = details.item.year {
                meta.push(year.to_string());
            }
            if let Some(subtitle) = &details.item.subtitle {
                meta.push(subtitle.clone());
            }
            widgets::muted_text(ui, &meta.join(" · "), palette);

            if let Some(loaded) = &details.details {
                if !loaded.genres.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        for genre in &loaded.genres {
                            widgets::chip(ui, genre, false, palette);
                        }
                    });
                }
            }

            let bookmarked = state.library.is_bookmarked(&details.item.key());
            ui.horizontal(|ui| {
                let label = if details.sources.is_empty() {
                    "Find sources"
                } else {
                    "Play"
                };
                if widgets::icon_button(
                    ui,
                    Icon::Play,
                    Some(label),
                    Vec2::new(96.0, 34.0),
                    false,
                    palette,
                    true,
                )
                .clicked()
                {
                    if details.sources.is_empty() {
                        intents.push(Intent::RequestSources {
                            item: Box::new(details.item.clone()),
                            episode_id: None,
                        });
                    } else {
                        intents.push(Intent::PlaySource { source_index: 0 });
                    }
                }
                if widgets::icon_button(
                    ui,
                    Icon::Bookmark,
                    None,
                    Vec2::splat(34.0),
                    bookmarked,
                    palette,
                    false,
                )
                .clicked()
                {
                    intents.push(Intent::ToggleBookmark(Box::new(details.item.clone())));
                }
            });

            if details.loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    widgets::muted_text(ui, "Loading details…", palette);
                });
            }
            if let Some(loaded) = &details.details {
                if let Some(description) = &loaded.description {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(description)
                            .size(13.0)
                            .color(palette.text_dim),
                    );
                }
            }
            if let Some(error) = &details.error {
                ui.label(RichText::new(error).size(12.5).color(palette.danger));
            }
        });
    });
}

fn sources(
    ui: &mut Ui,
    _state: &AppState,
    details: &app_core::state::DetailsState,
    palette: Palette,
    intents: &mut Vec<Intent>,
) {
    if details.resolving {
        ui.horizontal(|ui| {
            ui.spinner();
            widgets::muted_text(ui, "Resolving stream sources…", palette);
        });
        return;
    }
    if details.sources.is_empty() {
        return;
    }

    widgets::section_title(ui, "Sources", palette);
    for (index, source) in details.sources.iter().enumerate() {
        let frame = egui::Frame::default()
            .fill(palette.surface_alt)
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::symmetric(10, 7));
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(source.display_label())
                        .size(13.0)
                        .color(palette.text),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::icon_button(
                        ui,
                        Icon::Play,
                        None,
                        Vec2::splat(28.0),
                        false,
                        palette,
                        false,
                    )
                    .clicked()
                    {
                        intents.push(Intent::PlaySource {
                            source_index: index,
                        });
                    }
                });
            });
        });
        ui.add_space(4.0);
    }
}

fn episodes(
    ui: &mut Ui,
    details: &app_core::state::DetailsState,
    episodes: &[app_core::Episode],
    palette: Palette,
    intents: &mut Vec<Intent>,
) {
    widgets::section_title(ui, "Episodes", palette);
    for episode in episodes {
        let label = episode.label();
        let response = ui.add(
            egui::Button::new(RichText::new(label).size(13.0).color(palette.text))
                .min_size(Vec2::new(ui.available_width(), 30.0)),
        );
        if response.clicked() {
            intents.push(Intent::RequestSources {
                item: Box::new(details.item.clone()),
                episode_id: Some(episode.id.clone()),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Player
// ---------------------------------------------------------------------------

pub fn player(
    ctx: &egui::Context,
    state: &AppState,
    engine: Option<EngineHandle>,
    palette: Palette,
    video: &mut VideoSurface,
    intents: &mut Vec<Intent>,
) {
    let screen = ctx.screen_rect();
    ctx.layer_painter(egui::LayerId::new(Order::Foreground, Id::new("player_bg")))
        .rect_filled(screen, CornerRadius::ZERO, Color32::BLACK);

    egui::Area::new(Id::new("player_root"))
        .order(Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            ui.set_width(screen.width());
            ui.set_height(screen.height());

            let player = &state.player;

            // -- top strip -------------------------------------------------
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(
                    RichText::new(
                        player
                            .item
                            .as_ref()
                            .map(|item| item.title.as_str())
                            .unwrap_or("Player"),
                    )
                    .size(15.0)
                    .color(Color32::from_gray(220)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::icon_button(
                        ui,
                        Icon::Close,
                        None,
                        Vec2::splat(30.0),
                        false,
                        palette,
                        false,
                    )
                    .clicked()
                    {
                        intents.push(Intent::ClosePlayer);
                    }
                    ui.add_space(8.0);
                    if widgets::icon_button(
                        ui,
                        Icon::Bookmark,
                        None,
                        Vec2::splat(30.0),
                        player
                            .item
                            .as_ref()
                            .map(|item| state.library.is_bookmarked(&item.key()))
                            .unwrap_or(false),
                        palette,
                        false,
                    )
                    .clicked()
                    {
                        if let Some(item) = &player.item {
                            intents.push(Intent::ToggleBookmark(Box::new(item.clone())));
                        }
                    }
                });
            });

            // -- video -----------------------------------------------------
            let controls_height = 96.0;
            let video_size = Vec2::new(
                screen.width(),
                (screen.height() - controls_height - 40.0).max(120.0),
            );
            let (video_rect, response) = ui.allocate_exact_size(video_size, Sense::click());
            video.paint(ui, video_rect, engine, player.engine, palette);
            if response.clicked() {
                intents.push(Intent::PlayerCommand(PlaybackCommand::TogglePause));
            }

            if player.buffering {
                ui.painter().text(
                    video_rect.right_top() + Vec2::new(-90.0, 22.0),
                    Align2::RIGHT_TOP,
                    "Buffering…",
                    egui::FontId::proportional(12.5),
                    Color32::from_gray(200),
                );
            }
            if let PlayerPhase::Failed(message) = &player.phase {
                ui.painter().text(
                    video_rect.center(),
                    Align2::CENTER_CENTER,
                    format!("Playback failed: {message}"),
                    egui::FontId::proportional(14.0),
                    palette.danger,
                );
            }

            // -- controls --------------------------------------------------
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                widgets::muted_text(ui, &widgets::format_time(player.position), palette);
                let fraction = player.progress_fraction();
                if let Some(new_fraction) = widgets::seek_bar(ui, fraction, 18.0, palette) {
                    let target = new_fraction as f64 * player.duration;
                    intents.push(Intent::SeekTo(target));
                }
                widgets::muted_text(ui, &widgets::format_time(player.duration), palette);
                ui.add_space(12.0);
            });

            ui.horizontal(|ui| {
                ui.add_space(12.0);
                let playing = player.phase == PlayerPhase::Playing;
                if widgets::icon_button(
                    ui,
                    if playing { Icon::Pause } else { Icon::Play },
                    None,
                    Vec2::splat(40.0),
                    false,
                    palette,
                    true,
                )
                .clicked()
                {
                    intents.push(Intent::PlayerCommand(PlaybackCommand::TogglePause));
                }
                if widgets::icon_button(
                    ui,
                    Icon::Refresh,
                    None,
                    Vec2::splat(32.0),
                    false,
                    palette,
                    false,
                )
                .clicked()
                {
                    intents.push(Intent::SeekRelative(-10.0));
                }
                ui.add_space(6.0);

                // Volume
                let mut volume = player.volume;
                if ui
                    .add(
                        egui::Slider::new(&mut volume, 0.0..=1.0)
                            .show_value(false)
                            .trailing_fill(true),
                    )
                    .changed()
                {
                    intents.push(Intent::SetVolume(volume));
                }
                if widgets::icon_button(
                    ui,
                    if player.muted {
                        Icon::Close
                    } else {
                        Icon::Play
                    },
                    None,
                    Vec2::splat(28.0),
                    false,
                    palette,
                    false,
                )
                .clicked()
                {
                    intents.push(Intent::ToggleMute);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(12.0);
                    for mode in AspectMode::all() {
                        if widgets::chip(ui, mode.label(), player.aspect == mode, palette).clicked()
                        {
                            intents.push(Intent::SetAspect(mode));
                        }
                    }
                    widgets::muted_text(ui, "Fit", palette);
                });
            });
        });
}
