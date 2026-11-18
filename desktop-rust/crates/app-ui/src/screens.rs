//! The main screens: home, providers, search, bookmarks, extensions, settings.
//!
//! Every function here is a pure projection of [`AppState`] into widgets plus a
//! list of [`Intent`]s. Nothing blocks, nothing mutates state, nothing touches
//! the network — which is why the whole UI can be exercised by feeding it states.

use crate::covers::CoverCache;
use crate::theme::Palette;
use crate::widgets::{self, Icon};
use app_core::{
    AppState, AspectMode, Breakpoint, Intent, LayoutMetrics, MediaItem, Quality, Route, Settings,
    Theme,
};
use egui::{Ui, Vec2};

pub fn render(
    ui: &mut Ui,
    state: &AppState,
    layout: LayoutMetrics,
    palette: Palette,
    covers: &mut CoverCache,
    intents: &mut Vec<Intent>,
) {
    // The UI is the only place that knows the pointer state; reports it as
    // breakpoint hint once per frame via the window metrics already in state.
    debug_assert!(layout.columns >= 1);
    match state.route {
        Route::Home => home(ui, state, layout, palette, covers, intents),
        Route::Browse => providers(ui, state, layout, palette, intents),
        Route::Search => search(ui, state, layout, palette, covers, intents),
        Route::Bookmarks => bookmarks(ui, state, palette, covers, intents),
        Route::Extensions => extensions(ui, state, palette, intents),
        Route::Settings => settings(ui, state, palette, intents),
    }
}

// ---------------------------------------------------------------------------
// Home
// ---------------------------------------------------------------------------

fn home(
    ui: &mut Ui,
    state: &AppState,
    layout: LayoutMetrics,
    palette: Palette,
    covers: &mut CoverCache,
    intents: &mut Vec<Intent>,
) {
    if state.browse.providers.is_empty() {
        let clicked = widgets::empty_state(
            ui,
            Icon::Extensions,
            "Nothing to watch yet",
            "This player ships with no content. Add a repository in Extensions and install the sources you want.",
            palette,
            Some("Open Extensions"),
        );
        if clicked {
            intents.push(Intent::Navigate(Route::Extensions));
        }
        return;
    }

    header(ui, "Home", palette);

    let continue_watching = state.library.continue_watching(12);
    if !continue_watching.is_empty() {
        widgets::section_title(ui, "Continue watching", palette);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(layout.gutter, layout.gutter);
            for progress in continue_watching {
                let item = &progress.item;
                let cover = item
                    .poster
                    .as_ref()
                    .and_then(|url| covers.get(ui.ctx(), url));
                let bookmarked = state.library.is_bookmarked(&item.key());
                let card = widgets::poster_card(
                    ui,
                    item,
                    widgets::CardSpec {
                        width: layout.card_width,
                        poster_height: layout.poster_height,
                        cover,
                        progress: Some(progress.fraction()),
                        bookmarked,
                        palette,
                    },
                );
                if card.response.clicked() {
                    intents.push(Intent::OpenDetails(Box::new(item.clone())));
                }
            }
        });
        ui.add_space(layout.gutter);
    }

    if state.library.rows.is_empty() {
        if state.library.loading {
            ui.add_space(24.0);
            ui.horizontal(|ui| {
                ui.spinner();
                widgets::muted_text(ui, "Asking every provider for its home screen…", palette);
            });
        } else {
            let clicked = widgets::empty_state(
                ui,
                Icon::Refresh,
                "No shelves yet",
                "Your installed extensions did not return a home screen. Try the Providers tab or search instead.",
                palette,
                Some("Refresh"),
            );
            if clicked {
                intents.push(Intent::RefreshHome);
            }
        }
        return;
    }

    for row in &state.library.rows {
        widgets::section_title(ui, &row.title, palette);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(layout.gutter, layout.gutter);
            for item in &row.items {
                let cover = item
                    .poster
                    .as_ref()
                    .and_then(|url| covers.get(ui.ctx(), url));
                let bookmarked = state.library.is_bookmarked(&item.key());
                let progress = state
                    .library
                    .progress_for(&item.key())
                    .map(|p| p.fraction());
                let card = widgets::poster_card(
                    ui,
                    item,
                    widgets::CardSpec {
                        width: layout.card_width,
                        poster_height: layout.poster_height,
                        cover,
                        progress,
                        bookmarked,
                        palette,
                    },
                );
                if card.response.clicked() {
                    intents.push(Intent::OpenDetails(Box::new(item.clone())));
                }
            }
        });
        ui.add_space(layout.gutter * 0.5);
    }
}

// ---------------------------------------------------------------------------
// Providers
// ---------------------------------------------------------------------------

fn providers(
    ui: &mut Ui,
    state: &AppState,
    layout: LayoutMetrics,
    palette: Palette,
    intents: &mut Vec<Intent>,
) {
    header(ui, "Providers", palette);

    if state.browse.providers.is_empty() {
        let clicked = widgets::empty_state(
            ui,
            Icon::Browse,
            "No providers installed",
            "Providers come from extensions. Add a repository URL in the Extensions tab to see what is available.",
            palette,
            Some("Manage extensions"),
        );
        if clicked {
            intents.push(Intent::Navigate(Route::Extensions));
        }
        return;
    }

    let card_width = (layout.content_width(state.window_width) / 2.0 - layout.gutter).max(240.0);
    for provider in &state.browse.providers {
        let installed = state
            .extensions
            .installed
            .iter()
            .find(|e| e.provider_id == provider.id);
        widgets::card_frame(palette, 10, 14).show(ui, |ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(card_width, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(&provider.name)
                                .size(15.5)
                                .color(palette.text),
                        );
                        ui.add_space(6.0);
                        widgets::chip(ui, &format!("v{}", provider.version), false, palette);
                        if installed.map(|e| e.enabled).unwrap_or(false) {
                            widgets::chip(ui, "Enabled", true, palette);
                        }
                    });
                    if !provider.description.is_empty() {
                        widgets::muted_text(ui, &provider.description, palette);
                    }
                    let caps = &provider.capabilities;
                    let mut capabilities = Vec::new();
                    if caps.home {
                        capabilities.push("home");
                    }
                    if caps.search {
                        capabilities.push("search");
                    }
                    if caps.details {
                        capabilities.push("details");
                    }
                    if caps.live {
                        capabilities.push("live");
                    }
                    if !capabilities.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            for capability in capabilities {
                                widgets::chip(ui, capability, false, palette);
                            }
                        });
                    }
                },
            );
        });
        ui.add_space(layout.gutter * 0.75);
    }
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

fn search(
    ui: &mut Ui,
    state: &AppState,
    layout: LayoutMetrics,
    palette: Palette,
    covers: &mut CoverCache,
    intents: &mut Vec<Intent>,
) {
    header(ui, "Search", palette);

    let mut query = state.search.query.clone();
    let response = ui.add(
        egui::TextEdit::singleline(&mut query)
            .hint_text("Search all installed providers…")
            .desired_width(ui.available_width().min(520.0)),
    );
    if query != state.search.query {
        intents.push(Intent::SearchInput(query.clone()));
    }
    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        intents.push(Intent::SearchSubmit(query));
    }
    ui.add_space(6.0);

    if state.search.is_pending() {
        ui.horizontal(|ui| {
            ui.spinner();
            widgets::muted_text(
                ui,
                &format!("Waiting for {} provider(s)…", state.search.pending.len()),
                palette,
            );
        });
    }
    for (provider_id, error) in &state.search.failures {
        widgets::muted_text(ui, &format!("{provider_id}: {error}"), palette);
    }

    if state.search.results.is_empty() {
        if !state.search.active_query.is_empty() && !state.search.is_pending() {
            widgets::empty_state(
                ui,
                Icon::Search,
                "No results",
                &format!("Nothing matched “{}”.", state.search.active_query),
                palette,
                None,
            );
        } else if state.browse.providers.is_empty() {
            widgets::empty_state(
                ui,
                Icon::Extensions,
                "Search needs providers",
                "Install at least one extension to search across sources.",
                palette,
                None,
            );
        } else {
            widgets::empty_state(
                ui,
                Icon::Search,
                "Search everything at once",
                "One query is fanned out to every installed provider; results appear as they arrive.",
                palette,
                None,
            );
        }
        return;
    }

    ui.add_space(6.0);
    result_grid(
        ui,
        state,
        layout,
        palette,
        covers,
        &state.search.results,
        intents,
    );
}

fn result_grid(
    ui: &mut Ui,
    state: &AppState,
    layout: LayoutMetrics,
    palette: Palette,
    covers: &mut CoverCache,
    items: &[MediaItem],
    intents: &mut Vec<Intent>,
) {
    for chunk in items.chunks(layout.columns.max(1)) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(layout.gutter, layout.gutter);
            for item in chunk {
                let cover = item
                    .poster
                    .as_ref()
                    .and_then(|url| covers.get(ui.ctx(), url));
                let bookmarked = state.library.is_bookmarked(&item.key());
                let card = widgets::poster_card(
                    ui,
                    item,
                    widgets::CardSpec {
                        width: layout.card_width,
                        poster_height: layout.poster_height,
                        cover,
                        progress: None,
                        bookmarked,
                        palette,
                    },
                );
                if card.response.clicked() {
                    intents.push(Intent::OpenDetails(Box::new(item.clone())));
                }
            }
        });
        ui.add_space(layout.gutter * 0.9);
    }
}

// ---------------------------------------------------------------------------
// Bookmarks
// ---------------------------------------------------------------------------

fn bookmarks(
    ui: &mut Ui,
    state: &AppState,
    palette: Palette,
    covers: &mut CoverCache,
    intents: &mut Vec<Intent>,
) {
    header(ui, "Bookmarks", palette);

    if state.library.bookmarks.is_empty() {
        widgets::empty_state(
            ui,
            Icon::Bookmark,
            "Nothing saved yet",
            "Use the bookmark button on any title to keep it here. Bookmarks are stored locally, never synced.",
            palette,
            None,
        );
        return;
    }

    for bookmark in &state.library.bookmarks {
        widgets::card_frame(palette, 9, 10).show(ui, |ui| {
            ui.horizontal(|ui| {
                let thumb = Vec2::new(46.0, 66.0);
                let (rect, response) = ui.allocate_exact_size(thumb, egui::Sense::click());
                match bookmark
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
                            egui::Color32::WHITE,
                        );
                    }
                    None => {
                        let (top, _) = crate::theme::placeholder_tint(&bookmark.item.key());
                        ui.painter()
                            .rect_filled(rect, egui::CornerRadius::same(6), top);
                    }
                }
                if response.clicked() {
                    intents.push(Intent::OpenDetails(Box::new(bookmark.item.clone())));
                }

                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(&bookmark.item.title)
                            .size(14.5)
                            .color(palette.text),
                    );
                    let subtitle = bookmark
                        .item
                        .subtitle
                        .clone()
                        .unwrap_or_else(|| bookmark.item.kind.label().to_string());
                    widgets::muted_text(ui, &subtitle, palette);
                    if let Some(progress) = state.library.progress_for(&bookmark.item.key()) {
                        widgets::muted_text(
                            ui,
                            &format!(
                                "{} of {}",
                                widgets::format_time(progress.position),
                                widgets::format_time(progress.duration)
                            ),
                            palette,
                        );
                    }
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Remove").clicked() {
                        intents.push(Intent::RemoveBookmark(bookmark.item.key()));
                    }
                    if ui.button("Open").clicked() {
                        intents.push(Intent::OpenDetails(Box::new(bookmark.item.clone())));
                    }
                });
            });
        });
        ui.add_space(6.0);
    }
}

// ---------------------------------------------------------------------------
// Extensions
// ---------------------------------------------------------------------------

fn extensions(ui: &mut Ui, state: &AppState, palette: Palette, intents: &mut Vec<Intent>) {
    header(ui, "Extensions", palette);
    widgets::muted_text(
        ui,
        "Built-in providers are compiled into this build and need no download. Repositories add WebAssembly extensions, which must be built for Windows.",
        palette,
    );
    ui.add_space(10.0);

    // -- built-in providers -------------------------------------------------
    if !state.extensions.builtins.is_empty() {
        widgets::section_title(ui, "Built-in providers", palette);
        for builtin in &state.extensions.builtins {
            widgets::card_frame(palette, 8, 10).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&builtin.name)
                            .size(14.0)
                            .color(palette.text),
                    );
                    widgets::chip(ui, &format!("v{}", builtin.version), false, palette);
                    widgets::chip(ui, &builtin.capabilities, false, palette);
                    widgets::chip(
                        ui,
                        if builtin.enabled {
                            "Enabled"
                        } else {
                            "Disabled"
                        },
                        builtin.enabled,
                        palette,
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = if builtin.testing {
                            "Testing…"
                        } else {
                            "Test"
                        };
                        if ui
                            .add_enabled(!builtin.testing, egui::Button::new(label))
                            .on_hover_text("Search the live site once and report what came back")
                            .clicked()
                        {
                            intents.push(Intent::TestBuiltin(builtin.id.clone()));
                        }
                        let toggle = if builtin.enabled { "Disable" } else { "Enable" };
                        if ui.button(toggle).clicked() {
                            intents.push(Intent::SetBuiltinEnabled {
                                id: builtin.id.clone(),
                                enabled: !builtin.enabled,
                            });
                        }
                    });
                });
                widgets::muted_text(ui, &builtin.description, palette);
                widgets::muted_text(ui, &format!("Source: {}", builtin.origin), palette);
                if let Some(test) = &builtin.last_test {
                    let color = if test.ok {
                        palette.success
                    } else {
                        palette.danger
                    };
                    ui.label(egui::RichText::new(&test.detail).size(12.0).color(color));
                }
            });
            ui.add_space(6.0);
        }
        ui.add_space(10.0);
    }

    // -- add a repository ---------------------------------------------------
    let mut draft = state.extensions.draft_repository.clone();
    ui.horizontal(|ui| {
        let response = ui.add(
            egui::TextEdit::singleline(&mut draft)
                .hint_text("https://example.com/repository.json")
                .desired_width(ui.available_width().min(420.0)),
        );
        if draft != state.extensions.draft_repository {
            intents.push(Intent::RepositoryDraftChanged(draft.clone()));
        }
        let submit = ui.button("Add repository").clicked()
            || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
        if submit {
            intents.push(Intent::AddRepository(draft));
        }
        if state.extensions.busy {
            ui.spinner();
        }
    });

    // -- configured repositories -------------------------------------------
    if !state.extensions.repositories.is_empty() {
        ui.add_space(12.0);
        widgets::section_title(ui, "Repositories", palette);
        for repository in &state.extensions.repositories {
            widgets::card_frame(palette, 8, 10).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&repository.name)
                            .size(14.0)
                            .color(palette.text),
                    );
                    widgets::muted_text(
                        ui,
                        &format!("{} extension(s)", repository.extension_count),
                        palette,
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(!state.extensions.busy, egui::Button::new("Remove"))
                            .clicked()
                        {
                            intents.push(Intent::RemoveRepository(repository.url.clone()));
                        }
                        if ui
                            .add_enabled(!state.extensions.busy, egui::Button::new("Refresh"))
                            .clicked()
                        {
                            intents.push(Intent::RefreshRepository(repository.url.clone()));
                        }
                    });
                });
                widgets::muted_text(ui, &repository.url, palette);
                if let Some(error) = &repository.last_error {
                    ui.label(egui::RichText::new(error).size(12.0).color(palette.danger));
                }
            });
            ui.add_space(6.0);
        }
    }

    // -- available extensions ----------------------------------------------
    if !state.extensions.available.is_empty() {
        ui.add_space(10.0);
        widgets::section_title(ui, "Available", palette);
        for available in &state.extensions.available {
            let entry = &available.entry;
            let installed = state.extensions.is_installed(&entry.provider_id);
            widgets::card_frame(palette, 8, 10).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(&entry.name)
                                    .size(14.0)
                                    .color(palette.text),
                            );
                            widgets::chip(ui, &format!("v{}", entry.version), false, palette);
                            if let Some(language) = &entry.language {
                                widgets::chip(ui, language, false, palette);
                            }
                        });
                        if !entry.description.is_empty() {
                            widgets::muted_text(ui, &entry.description, palette);
                        }
                        widgets::muted_text(
                            ui,
                            &format!("sha256 {}", short_hash(&entry.sha256)),
                            palette,
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if installed {
                            if ui.button("Remove").clicked() {
                                intents.push(Intent::RemoveExtension(entry.provider_id.clone()));
                            }
                        } else if ui
                            .add_enabled(!state.extensions.busy, egui::Button::new("Install"))
                            .clicked()
                        {
                            intents.push(Intent::InstallExtension {
                                repository: available.repository.clone(),
                                provider_id: entry.provider_id.clone(),
                            });
                        }
                    });
                });
            });
            ui.add_space(6.0);
        }
    }

    // -- installed extensions ----------------------------------------------
    ui.add_space(10.0);
    widgets::section_title(ui, "Installed", palette);
    if state.extensions.installed.is_empty() {
        widgets::muted_text(ui, "No extensions installed.", palette);
    }
    for installed in &state.extensions.installed {
        let frame = egui::Frame::default()
            .fill(palette.surface)
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::same(10));
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(&installed.name)
                                .size(14.0)
                                .color(palette.text),
                        );
                        widgets::chip(ui, &format!("v{}", installed.version), false, palette);
                    });
                    widgets::muted_text(ui, &installed.wasm_path, palette);
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Remove").clicked() {
                        intents.push(Intent::RemoveExtension(installed.provider_id.clone()));
                    }
                    let label = if installed.enabled {
                        "Disable"
                    } else {
                        "Enable"
                    };
                    if ui.button(label).clicked() {
                        intents.push(Intent::SetExtensionEnabled {
                            provider_id: installed.provider_id.clone(),
                            enabled: !installed.enabled,
                        });
                    }
                });
            });
        });
        ui.add_space(6.0);
    }
}

fn short_hash(hash: &str) -> String {
    if hash.len() <= 12 {
        hash.to_string()
    } else {
        format!("{}…", &hash[..12])
    }
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

fn settings(ui: &mut Ui, state: &AppState, palette: Palette, intents: &mut Vec<Intent>) {
    header(ui, "Settings", palette);
    let mut draft: Settings = state.settings.clone();
    let mut changed = false;

    widgets::section_title(ui, "Appearance", palette);
    ui.horizontal(|ui| {
        widgets::muted_text(ui, "Theme", palette);
        for theme in Theme::all() {
            if widgets::chip(ui, theme.label(), draft.theme == theme, palette).clicked() {
                draft.theme = theme;
                changed = true;
            }
        }
    });
    ui.horizontal(|ui| {
        widgets::muted_text(ui, "Layout override", palette);
        if widgets::chip(ui, "Auto", draft.force_layout.is_none(), palette).clicked() {
            draft.force_layout = None;
            changed = true;
        }
        for breakpoint in Breakpoint::all() {
            if widgets::chip(
                ui,
                breakpoint.label(),
                draft.force_layout == Some(breakpoint),
                palette,
            )
            .clicked()
            {
                draft.force_layout = Some(breakpoint);
                changed = true;
            }
        }
    });
    if ui
        .checkbox(&mut draft.reduce_motion, "Reduce motion")
        .changed()
    {
        changed = true;
    }

    ui.add_space(8.0);
    widgets::section_title(ui, "Playback", palette);
    ui.horizontal(|ui| {
        widgets::muted_text(ui, "Video fit", palette);
        for mode in AspectMode::all() {
            if widgets::chip(ui, mode.label(), draft.aspect == mode, palette).clicked() {
                draft.aspect = mode;
                changed = true;
            }
        }
    });
    ui.horizontal(|ui| {
        widgets::muted_text(ui, "Preferred quality", palette);
        for quality in [
            Quality::Auto,
            Quality::Fhd1080,
            Quality::Hd720,
            Quality::Sd480,
        ] {
            if widgets::chip(
                ui,
                quality.label(),
                draft.preferred_quality == quality,
                palette,
            )
            .clicked()
            {
                draft.preferred_quality = quality;
                changed = true;
            }
        }
    });
    if ui
        .checkbox(&mut draft.resume_playback, "Resume where I left off")
        .changed()
    {
        changed = true;
    }
    if ui
        .checkbox(&mut draft.hardware_decoding, "Hardware decoding")
        .changed()
    {
        changed = true;
    }

    ui.add_space(8.0);
    widgets::section_title(ui, "Storage & network", palette);
    ui.horizontal(|ui| {
        widgets::muted_text(ui, "Cache limit (MB)", palette);
        let mut limit = draft.cache_limit_mb as f64;
        if ui
            .add(egui::Slider::new(&mut limit, 128.0..=8192.0).show_value(true))
            .changed()
        {
            draft.cache_limit_mb = limit as u32;
            changed = true;
        }
    });
    ui.horizontal(|ui| {
        widgets::muted_text(ui, "Network timeout (s)", palette);
        let mut timeout = draft.network_timeout_secs as f64;
        if ui
            .add(egui::Slider::new(&mut timeout, 5.0..=120.0).show_value(true))
            .changed()
        {
            draft.network_timeout_secs = timeout as u64;
            changed = true;
        }
    });

    ui.add_space(8.0);
    widgets::section_title(ui, "Diagnostics", palette);
    widgets::muted_text(
        ui,
        &format!("Playback engine: {}", state.backend.engine_detail),
        palette,
    );
    widgets::muted_text(
        ui,
        &format!(
            "WASM runtime: {}",
            if state.backend.wasm_runtime {
                "available"
            } else {
                "unavailable"
            }
        ),
        palette,
    );
    widgets::muted_text(
        ui,
        &format!("Data folder: {}", state.backend.data_dir),
        palette,
    );
    widgets::muted_text(
        ui,
        &format!(
            "Window: {:.0}×{:.0} @ {:.2}x — {} layout",
            state.window_width,
            state.window_height,
            state.dpi_scale,
            state.layout().breakpoint.label()
        ),
        palette,
    );

    ui.add_space(8.0);
    widgets::section_title(ui, "Privacy", palette);
    widgets::muted_text(
        ui,
        "No ads, no telemetry, no accounts. The app only contacts the repository URLs and stream hosts your extensions choose.",
        palette,
    );

    if changed {
        intents.push(Intent::UpdateSettings(Box::new(draft)));
    }
}

fn header(ui: &mut Ui, title: &str, palette: Palette) {
    ui.add_space(2.0);
    ui.label(
        egui::RichText::new(title)
            .size(21.0)
            .strong()
            .color(palette.text),
    );
    ui.add_space(8.0);
}
