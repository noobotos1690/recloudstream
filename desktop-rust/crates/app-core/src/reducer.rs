//! The reducer: the only function that mutates [`AppState`].
//!
//! Signature is deliberately boring — `(state, intent, ...) -> effects` — so
//! every transition is reachable from a unit test with no I/O. Anything that
//! would block is returned as an [`Effect`] instead of being performed here.

use crate::domain::{
    Bookmark, HomeRow, InstalledExtension, MediaDetails, MediaItem, PlaybackProgress, Repository,
    RepositoryRecord, StreamSource,
};
use crate::effect::Effect;
use crate::intent::Intent;
use crate::ports::{PlaybackCommand, PlaybackEvent, PlaybackRequest};
use crate::state::{
    AppState, DetailsState, PersistedSnapshot, PlayerPhase, Route, Settings, ToastKind,
};

/// How many home shelves we keep; extensions occasionally return dozens.
const MAX_ROWS: usize = 24;
/// Resume points beyond this many are pruned, oldest first.
const MAX_PROGRESS: usize = 400;

pub fn reduce(state: &mut AppState, intent: Intent) -> Vec<Effect> {
    let mut effects = Vec::new();

    match intent {
        // -- navigation & shell --------------------------------------------
        Intent::Navigate(route) => {
            state.route = route;
            // Navigating away dismisses the details overlay; the player is an
            // overlay of its own and survives (music continues while browsing).
            state.details = None;
        }
        Intent::Back => {
            if state.player.visible {
                close_player(state, &mut effects);
            } else if state.details.is_some() {
                state.details = None;
            } else if state.route != Route::Home {
                state.route = Route::Home;
            }
        }
        Intent::Resize { width, height, dpi } => {
            state.window_width = width.max(1.0);
            state.window_height = height.max(1.0);
            state.dpi_scale = if dpi > 0.0 { dpi } else { 1.0 };
            state.breakpoint = state.layout().breakpoint;
        }
        Intent::Notice { kind, message } => state.push_toast(kind, message),
        Intent::DismissToast(index) => {
            if index < state.toasts.len() {
                state.toasts.remove(index);
            }
        }

        // -- catalog --------------------------------------------------------
        Intent::RefreshHome => {
            if state.browse.providers.is_empty() {
                state.push_toast(
                    ToastKind::Info,
                    "Install an extension to start loading a home screen.",
                );
            } else {
                state.library.loading = true;
                effects.push(Effect::LoadHome);
            }
        }
        Intent::HomeLoaded(rows) => {
            state.library.rows = merge_rows(&state.library.rows, rows);
            state.library.loading = false;
            state.library.loaded_once = true;
        }
        Intent::HomeFailed(error) => {
            state.library.loading = false;
            state.push_toast(ToastKind::Error, format!("Home feed failed: {error}"));
        }

        // -- search ---------------------------------------------------------
        Intent::SearchInput(query) => state.search.query = query,
        Intent::SearchSubmit(query) => {
            let query = query.trim().to_string();
            state.search.query = query.clone();
            state.search.clear_results();

            if query.is_empty() {
                state.push_toast(ToastKind::Info, "Type something to search for.");
                return effects;
            }

            let provider_ids: Vec<String> = state
                .browse
                .providers
                .iter()
                .filter(|p| p.capabilities.search)
                .map(|p| p.id.clone())
                .collect();

            if provider_ids.is_empty() {
                state.push_toast(
                    ToastKind::Warning,
                    "No search-capable extensions installed yet.",
                );
                return effects;
            }

            state.search.active_query = query.clone();
            state.search.pending = provider_ids.clone();
            state.route = Route::Search;
            effects.push(Effect::Search {
                query,
                provider_ids,
            });
        }
        Intent::SearchResults {
            provider_id,
            query,
            items,
        } => {
            // Stale replies from a previous query are dropped rather than
            // appended, otherwise typing quickly mixes two result sets.
            if query != state.search.active_query {
                return effects;
            }
            state.search.pending.retain(|p| p != &provider_id);
            let mut added = 0usize;
            for item in items {
                if !state
                    .search
                    .results
                    .iter()
                    .any(|existing| existing.key() == item.key())
                {
                    state.search.results.push(item);
                    added += 1;
                }
            }
            state.search.results_by_provider.push((provider_id, added));
        }
        Intent::SearchFailed {
            provider_id,
            query,
            error,
        } => {
            if query != state.search.active_query {
                return effects;
            }
            state.search.pending.retain(|p| p != &provider_id);
            state.search.failures.push((provider_id, error));
        }

        // -- details & resolution -------------------------------------------
        Intent::OpenDetails(item) => {
            let item = *item;
            effects.push(Effect::LoadDetails { item: item.clone() });
            state.details = Some(DetailsState::new(item));
        }
        Intent::CloseDetails => state.details = None,
        Intent::DetailsLoaded(details) => {
            let details: MediaDetails = *details;
            let mut request = None;
            if let Some(current) = state.details.as_mut() {
                if current.item.key() == details.item.key() {
                    current.item = details.item.clone();
                    current.details = Some(details.clone());
                    current.loading = false;
                    current.error = None;
                    // Chain the second step automatically: opening a title means
                    // the user wants something to play, and a second round trip
                    // on their click would only add latency.
                    current.resolving = true;
                    request = Some((
                        current.item.clone(),
                        details.episodes.first().map(|episode| episode.id.clone()),
                    ));
                }
            }
            if let Some((item, episode_id)) = request {
                effects.push(Effect::ResolveSources { item, episode_id });
            }
        }
        Intent::DetailsFailed(error) => {
            if let Some(current) = state.details.as_mut() {
                current.loading = false;
                current.error = Some(error.clone());
            }
            state.push_toast(ToastKind::Error, format!("Could not load details: {error}"));
        }
        Intent::RequestSources { item, episode_id } => {
            let item = *item;
            if let Some(current) = state.details.as_mut() {
                current.resolving = true;
                current.error = None;
            }
            effects.push(Effect::ResolveSources { item, episode_id });
        }
        Intent::SourcesResolved { item, sources } => {
            let item = *item;
            let sources = sort_sources(sources);
            if let Some(current) = state.details.as_mut() {
                if current.item.key() == item.key() {
                    current.resolving = false;
                    if sources.is_empty() {
                        current.error = Some("No playable sources were returned.".to_string());
                    } else {
                        current.error = None;
                    }
                    current.sources = sources;
                }
            }
        }
        Intent::SourcesFailed(message) => {
            if let Some(current) = state.details.as_mut() {
                current.resolving = false;
                current.error = Some(message.clone());
            }
            state.push_toast(
                ToastKind::Error,
                format!("Could not resolve stream: {message}"),
            );
        }

        // -- playback -------------------------------------------------------
        Intent::PlaySource { source_index } => {
            let Some((item, sources)) = state.details.as_ref().and_then(|d| {
                d.sources
                    .get(source_index)
                    .map(|_| (d.item.clone(), d.sources.clone()))
            }) else {
                return effects;
            };
            let resume = resume_position(state, &item);
            start_playback(state, &mut effects, item, sources, source_index, resume);
        }
        Intent::ClosePlayer => close_player(state, &mut effects),
        Intent::PlayerCommand(command) => {
            // Optimistic phase flip so the button responds on the same frame the
            // decoder reports back on the next.
            if matches!(command, PlaybackCommand::TogglePause) {
                state.player.phase = if state.player.phase == PlayerPhase::Playing {
                    PlayerPhase::Paused
                } else {
                    PlayerPhase::Playing
                };
            }
            effects.push(Effect::PlayerCommand(command));
        }
        Intent::PlayerEvent(event) => match event {
            PlaybackEvent::Loaded { duration } => {
                if let Some(duration) = duration {
                    if duration.is_finite() && duration > 0.0 {
                        state.player.duration = duration;
                    }
                }
                state.player.buffering = false;
                if matches!(state.player.phase, PlayerPhase::Loading | PlayerPhase::Idle) {
                    state.player.phase = PlayerPhase::Playing;
                }
            }
            PlaybackEvent::Position { position, duration } => {
                if position.is_finite() && position >= 0.0 {
                    state.player.position = position;
                }
                if let Some(duration) = duration {
                    if duration.is_finite() && duration > 0.0 {
                        state.player.duration = duration;
                    }
                }
            }
            PlaybackEvent::Paused(paused) => {
                state.player.phase = if paused {
                    PlayerPhase::Paused
                } else {
                    PlayerPhase::Playing
                };
                effects.push(Effect::PersistProgress);
            }
            PlaybackEvent::Buffering(buffering) => state.player.buffering = buffering,
            PlaybackEvent::Ended => {
                state.player.phase = PlayerPhase::Ended;
                state.player.position = state.player.duration;
                record_progress(state, &mut effects);
            }
            PlaybackEvent::Failed(message) => {
                state.player.phase = PlayerPhase::Failed(message.clone());
                state.push_toast(ToastKind::Error, format!("Playback failed: {message}"));
            }
        },
        Intent::SetAspect(aspect) => {
            state.settings.aspect = aspect;
            state.player.aspect = aspect;
            effects.push(Effect::PlayerCommand(PlaybackCommand::SetAspect(aspect)));
            effects.push(Effect::PersistSettings);
        }
        Intent::SetVolume(volume) => {
            let volume = volume.clamp(0.0, 1.0);
            state.player.volume = volume;
            state.player.muted = volume == 0.0;
            effects.push(Effect::PlayerCommand(PlaybackCommand::SetVolume(volume)));
        }
        Intent::ToggleMute => {
            state.player.muted = !state.player.muted;
            effects.push(Effect::PlayerCommand(PlaybackCommand::SetMuted(
                state.player.muted,
            )));
        }
        Intent::SeekRelative(delta) => {
            let target = (state.player.position + delta).clamp(0.0, state.player.duration.max(0.0));
            state.player.position = target;
            effects.push(Effect::PlayerCommand(PlaybackCommand::SeekTo(target)));
        }
        Intent::SeekTo(position) => {
            let target = position.clamp(0.0, state.player.duration.max(0.0));
            state.player.position = target;
            effects.push(Effect::PlayerCommand(PlaybackCommand::SeekTo(target)));
        }
        Intent::PlayerReady { engine, detail } => {
            state.player.engine = engine;
            state.backend.engine = engine;
            state.backend.engine_detail = detail;
        }
        Intent::ProgressRecorded {
            item,
            episode_id,
            position,
            duration,
        } => {
            upsert_progress(state, *item, episode_id, position, duration);
            effects.push(Effect::PersistProgress);
        }

        // -- bookmarks -------------------------------------------------------
        Intent::ToggleBookmark(item) => {
            let item = *item;
            let key = item.key();
            if let Some(index) = state
                .library
                .bookmarks
                .iter()
                .position(|b| b.item.key() == key)
            {
                state.library.bookmarks.remove(index);
                state.push_toast(ToastKind::Info, format!("Removed “{}”", item.title));
            } else {
                state
                    .library
                    .bookmarks
                    .insert(0, Bookmark::new(item.clone(), now_unix()));
                state.push_toast(ToastKind::Success, format!("Saved “{}”", item.title));
            }
            effects.push(Effect::PersistBookmarks);
        }
        Intent::RemoveBookmark(key) => {
            state.library.bookmarks.retain(|b| b.item.key() != key);
            effects.push(Effect::PersistBookmarks);
        }

        // -- extensions ------------------------------------------------------
        Intent::RepositoryDraftChanged(value) => state.extensions.draft_repository = value,
        Intent::AddRepository(url) => {
            let url = url.trim().to_string();
            if !(url.starts_with("https://") || url.starts_with("http://")) {
                state.push_toast(
                    ToastKind::Warning,
                    "Repository URLs must start with http:// or https://",
                );
                return effects;
            }
            if state.extensions.repositories.iter().any(|r| r.url == url) {
                state.push_toast(ToastKind::Info, "That repository is already added.");
                return effects;
            }
            state.extensions.busy = true;
            state.extensions.draft_repository.clear();
            effects.push(Effect::FetchRepository { url });
        }
        Intent::RepositoryFetched { url, repository } => {
            let repository: Repository = *repository;
            state.extensions.busy = false;
            let record = RepositoryRecord {
                url: url.clone(),
                name: repository.name.clone(),
                fetched_at: now_unix(),
                extension_count: repository.extensions.len(),
                last_error: None,
            };
            match state
                .extensions
                .repositories
                .iter_mut()
                .find(|r| r.url == url)
            {
                Some(existing) => *existing = record,
                None => state.extensions.repositories.push(record),
            }
            // Replace this repository's slice of the available list wholesale.
            state.extensions.available.retain(|a| a.repository != url);
            for entry in repository.extensions {
                state
                    .extensions
                    .available
                    .push(crate::state::AvailableExtension {
                        repository: url.clone(),
                        entry,
                    });
            }
            state.push_toast(
                ToastKind::Success,
                format!(
                    "Loaded {} extension(s) from {}",
                    state.extensions.available_for(&url).len(),
                    url
                ),
            );
            effects.push(Effect::PersistRepositories);
        }
        Intent::RepositoryFailed { url, error } => {
            state.extensions.busy = false;
            // Read the old reason before overwriting it: a repository retried on
            // every start must not toast on every start.
            let repeated = state
                .extensions
                .repositories
                .iter()
                .find(|record| record.url == url)
                .and_then(|record| record.last_error.as_deref())
                .map(|previous| previous == error)
                .unwrap_or(false);
            let existing = state
                .extensions
                .repositories
                .iter_mut()
                .find(|r| r.url == url);
            match existing {
                Some(record) => record.last_error = Some(error.clone()),
                None => state.extensions.repositories.push(RepositoryRecord {
                    url: url.clone(),
                    name: url.clone(),
                    fetched_at: 0,
                    extension_count: 0,
                    last_error: Some(error.clone()),
                }),
            }
            if !repeated {
                state.push_toast(ToastKind::Error, format!("Repository failed: {error}"));
            }
            // Persist failures as well: a repository the build pre-added has to
            // keep explaining itself after a restart.
            effects.push(Effect::PersistRepositories);
        }
        Intent::RefreshRepository(url) => {
            if let Some(record) = state
                .extensions
                .repositories
                .iter_mut()
                .find(|r| r.url == url)
            {
                // Clear the old reason: the user asked for a retry, so the next
                // failure is new information and must be shown even if identical.
                record.last_error = None;
                state.extensions.busy = true;
                effects.push(Effect::FetchRepository { url });
            }
        }
        Intent::RemoveRepository(url) => {
            state
                .extensions
                .repositories
                .retain(|record| record.url != url);
            // Its offer list goes with it; installed extensions are files on disk
            // and stay installed, they simply lose their update source.
            state
                .extensions
                .available
                .retain(|entry| entry.repository != url);
            state.push_toast(ToastKind::Info, "Repository removed.");
            effects.push(Effect::PersistRepositories);
        }
        Intent::SetBuiltinEnabled { id, enabled } => {
            if let Some(builtin) = state.extensions.builtin_mut(&id) {
                builtin.enabled = enabled;
            }
            if enabled {
                state
                    .settings
                    .disabled_builtins
                    .retain(|entry| entry != &id);
            } else if !state.settings.disabled_builtins.contains(&id) {
                state.settings.disabled_builtins.push(id);
            }
            effects.push(Effect::PersistSettings);
            effects.push(Effect::ReloadProviders);
        }
        Intent::TestBuiltin(id) => {
            if let Some(builtin) = state.extensions.builtin_mut(&id) {
                builtin.testing = true;
                builtin.last_test = None;
            }
            effects.push(Effect::TestBuiltin(id));
        }
        Intent::BuiltinTested { id, ok, detail } => {
            if let Some(builtin) = state.extensions.builtin_mut(&id) {
                builtin.testing = false;
                builtin.last_test = Some(crate::state::BuiltinTest { ok, detail });
            }
        }
        Intent::InstallExtension {
            repository,
            provider_id,
        } => {
            let entry = state
                .extensions
                .available
                .iter()
                .find(|a| a.repository == repository && a.entry.provider_id == provider_id)
                .map(|a| a.entry.clone());
            match entry {
                Some(entry) => {
                    state.extensions.busy = true;
                    effects.push(Effect::InstallExtension {
                        repository,
                        entry: Box::new(entry),
                    });
                }
                None => state.push_toast(
                    ToastKind::Warning,
                    "That extension is no longer listed by its repository.",
                ),
            }
        }
        Intent::ExtensionInstalled(extension) => {
            let extension: InstalledExtension = *extension;
            state.extensions.busy = false;
            state
                .extensions
                .installed
                .retain(|e| e.provider_id != extension.provider_id);
            state.extensions.installed.push(extension.clone());
            state.push_toast(
                ToastKind::Success,
                format!("Installed {} {}", extension.name, extension.version),
            );
            effects.push(Effect::PersistExtensions);
            effects.push(Effect::ReloadProviders);
        }
        Intent::ExtensionFailed { provider_id, error } => {
            state.extensions.busy = false;
            state.push_toast(ToastKind::Error, format!("{provider_id}: {error}"));
        }
        Intent::SetExtensionEnabled {
            provider_id,
            enabled,
        } => {
            if let Some(record) = state
                .extensions
                .installed
                .iter_mut()
                .find(|e| e.provider_id == provider_id)
            {
                record.enabled = enabled;
            }
            effects.push(Effect::PersistExtensions);
            effects.push(Effect::ReloadProviders);
        }
        Intent::RemoveExtension(provider_id) => {
            if let Some(index) = state
                .extensions
                .installed
                .iter()
                .position(|e| e.provider_id == provider_id)
            {
                let removed = state.extensions.installed.remove(index);
                effects.push(Effect::DeleteExtensionFile {
                    wasm_path: removed.wasm_path,
                });
                effects.push(Effect::PersistExtensions);
                effects.push(Effect::ReloadProviders);
                state.push_toast(ToastKind::Info, format!("Removed {}", removed.name));
            }
        }
        Intent::ProvidersRefreshed(providers) => {
            state.browse.providers = providers;
            if state.library.loaded_once {
                // New provider installed: refresh the home feed quietly.
                state.library.loading = true;
                effects.push(Effect::LoadHome);
            }
        }

        // -- settings & bootstrap -------------------------------------------
        Intent::UpdateSettings(settings) => {
            let settings: Settings = *settings;
            state.player.aspect = settings.aspect;
            state.settings = settings;
            effects.push(Effect::PersistSettings);
        }
        Intent::Hydrate(snapshot) => {
            let snapshot: PersistedSnapshot = *snapshot;
            state.settings = snapshot.settings;
            state.player.aspect = state.settings.aspect;
            state.library.bookmarks = snapshot.bookmarks;
            state.library.progress = snapshot.progress;
            state.extensions.repositories = snapshot.repositories;
            state.extensions.installed = snapshot.extensions;
        }
    }

    // Any intent is worth a fresh frame; the runtime clears the flag after paint.
    state.needs_repaint = true;
    effects
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Best-first ordering, and stable for equal quality so providers keep their
/// own preference order.
fn sort_sources(mut sources: Vec<StreamSource>) -> Vec<StreamSource> {
    sources.sort_by_key(|source| std::cmp::Reverse(source.quality.rank()));
    sources
}

fn merge_rows(existing: &[HomeRow], incoming: Vec<HomeRow>) -> Vec<HomeRow> {
    let mut merged: Vec<HomeRow> = existing.to_vec();
    for row in incoming {
        if row.items.is_empty() {
            continue;
        }
        match merged.iter_mut().find(|r| r.title == row.title) {
            Some(slot) => {
                for item in row.items {
                    if !slot.items.iter().any(|e| e.key() == item.key()) {
                        slot.items.push(item);
                    }
                }
            }
            None => merged.push(row),
        }
    }
    merged.truncate(MAX_ROWS);
    merged
}

fn resume_position(state: &AppState, item: &MediaItem) -> f64 {
    if !state.settings.resume_playback {
        return 0.0;
    }
    state
        .library
        .progress_for(&item.key())
        .map(|p| {
            // Don't resume into the credits.
            if p.fraction() > 0.95 {
                0.0
            } else {
                p.position
            }
        })
        .unwrap_or(0.0)
}

fn start_playback(
    state: &mut AppState,
    effects: &mut Vec<Effect>,
    item: MediaItem,
    sources: Vec<StreamSource>,
    source_index: usize,
    resume: f64,
) {
    let Some(source) = sources.get(source_index).cloned() else {
        return;
    };
    let episode_id = state
        .details
        .as_ref()
        .filter(|d| d.item.key() == item.key())
        .and_then(|d| d.details.as_ref())
        .and_then(|d| d.episodes.first())
        .map(|e| e.id.clone());

    state.player.visible = true;
    state.player.item = Some(item.clone());
    state.player.episode_id = episode_id;
    state.player.subtitles = source.subtitles.clone();
    state.player.subtitle = source
        .subtitles
        .iter()
        .find(|s| s.default)
        .map(|s| s.url.clone());
    state.player.sources = sources;
    state.player.active_source = Some(source_index);
    state.player.phase = PlayerPhase::Loading;
    state.player.position = resume;
    state.player.duration = 0.0;
    state.player.resume_from = resume;
    state.player.buffering = true;
    state.details = None;

    effects.push(Effect::PlayerLoad {
        request: Box::new(PlaybackRequest {
            url: source.url,
            headers: source.headers,
            subtitles: state.player.subtitles.clone(),
            title: item.title,
            start_position: resume,
            live: source.live,
        }),
    });
}

fn close_player(state: &mut AppState, effects: &mut Vec<Effect>) {
    record_progress(state, effects);
    state.player.reset();
    state.player.visible = false;
    effects.push(Effect::PlayerStop);
}

fn record_progress(state: &mut AppState, effects: &mut Vec<Effect>) {
    let Some(item) = state.player.item.clone() else {
        return;
    };
    if state.player.duration <= 0.0 || state.player.position <= 0.0 {
        return;
    }
    let episode_id = state.player.episode_id.clone();
    let position = state.player.position;
    let duration = state.player.duration;
    upsert_progress(state, item, episode_id, position, duration);
    effects.push(Effect::PersistProgress);
}

fn upsert_progress(
    state: &mut AppState,
    item: MediaItem,
    episode_id: Option<String>,
    position: f64,
    duration: f64,
) {
    let key = item.key();
    let entry = PlaybackProgress {
        key: key.clone(),
        item,
        position,
        duration,
        updated_at: now_unix(),
        episode_id,
    };
    match state.library.progress.iter_mut().find(|p| p.key == key) {
        Some(slot) => *slot = entry,
        None => state.library.progress.push(entry),
    }
    if state.library.progress.len() > MAX_PROGRESS {
        state
            .library
            .progress
            .sort_by_key(|entry| std::cmp::Reverse(entry.updated_at));
        state.library.progress.truncate(MAX_PROGRESS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ProviderCapabilities, ProviderManifest, Quality};

    fn provider(id: &str, search: bool) -> ProviderManifest {
        ProviderManifest {
            id: id.to_string(),
            name: id.to_string(),
            version: "1.0.0".to_string(),
            description: String::new(),
            language: None,
            icon_url: None,
            capabilities: ProviderCapabilities {
                search,
                ..ProviderCapabilities::default()
            },
        }
    }

    fn item(id: &str) -> MediaItem {
        MediaItem::new("p1", id, format!("Title {id}"))
    }

    #[test]
    fn resize_updates_breakpoint() {
        let mut state = AppState::default();
        reduce(
            &mut state,
            Intent::Resize {
                width: 640.0,
                height: 900.0,
                dpi: 1.0,
            },
        );
        assert_eq!(state.breakpoint, crate::layout::Breakpoint::Compact);

        reduce(
            &mut state,
            Intent::Resize {
                width: 1500.0,
                height: 900.0,
                dpi: 1.0,
            },
        );
        assert_eq!(state.breakpoint, crate::layout::Breakpoint::Expanded);
    }

    #[test]
    fn search_without_providers_warns_and_does_nothing() {
        let mut state = AppState::default();
        let effects = reduce(&mut state, Intent::SearchSubmit("dune".into()));
        assert!(effects.is_empty());
        assert_eq!(state.toasts.len(), 1);
        assert!(state.search.results.is_empty());
    }

    #[test]
    fn search_fans_out_only_to_search_capable_providers() {
        let mut state = AppState::default();
        reduce(
            &mut state,
            Intent::ProvidersRefreshed(vec![provider("a", true), provider("b", false)]),
        );
        let effects = reduce(&mut state, Intent::SearchSubmit(" dune ".into()));
        let Effect::Search {
            query,
            provider_ids,
        } = &effects[0]
        else {
            panic!("expected search effect, got {effects:?}");
        };
        assert_eq!(query, "dune");
        assert_eq!(provider_ids, &vec!["a".to_string()]);
        assert_eq!(state.route, Route::Search);
        assert_eq!(state.search.pending, vec!["a".to_string()]);
    }

    #[test]
    fn stale_search_replies_are_dropped() {
        let mut state = AppState::default();
        reduce(
            &mut state,
            Intent::ProvidersRefreshed(vec![provider("a", true)]),
        );
        reduce(&mut state, Intent::SearchSubmit("first".into()));
        reduce(
            &mut state,
            Intent::SearchResults {
                provider_id: "a".into(),
                query: "old".into(),
                items: vec![item("1")],
            },
        );
        assert!(state.search.results.is_empty());
        assert_eq!(state.search.pending.len(), 1);

        reduce(
            &mut state,
            Intent::SearchResults {
                provider_id: "a".into(),
                query: "first".into(),
                items: vec![item("1"), item("1")],
            },
        );
        assert_eq!(state.search.results.len(), 1, "duplicates are collapsed");
        assert!(state.search.pending.is_empty());
    }

    #[test]
    fn bookmark_toggles_round_trip() {
        let mut state = AppState::default();
        let target = Box::new(item("9"));

        let effects = reduce(&mut state, Intent::ToggleBookmark(target.clone()));
        assert!(matches!(effects[0], Effect::PersistBookmarks));
        assert!(state.library.is_bookmarked(&target.key()));

        reduce(&mut state, Intent::ToggleBookmark(target.clone()));
        assert!(!state.library.is_bookmarked(&target.key()));
    }

    #[test]
    fn sources_are_sorted_best_quality_first() {
        let mut low = StreamSource::new("https://x/360.mp4");
        low.quality = Quality::Sd480;
        let mut high = StreamSource::new("https://x/1080.mp4");
        high.quality = Quality::Fhd1080;

        let mut state = AppState {
            details: Some(DetailsState::new(item("1"))),
            ..AppState::default()
        };
        reduce(
            &mut state,
            Intent::SourcesResolved {
                item: Box::new(item("1")),
                sources: vec![low, high],
            },
        );
        let sources = &state.details.as_ref().unwrap().sources;
        assert_eq!(sources[0].quality, Quality::Fhd1080);
    }

    #[test]
    fn playback_reaches_player_phase_and_requests_load() {
        let mut state = AppState::default();
        let target = item("1");
        state.details = Some(DetailsState {
            item: target.clone(),
            details: None,
            loading: false,
            resolving: false,
            sources: vec![StreamSource::new("https://cdn/x.m3u8")],
            error: None,
        });

        let effects = reduce(&mut state, Intent::PlaySource { source_index: 0 });
        assert!(matches!(effects[0], Effect::PlayerLoad { .. }));
        assert!(state.player.visible);
        assert_eq!(state.player.phase, PlayerPhase::Loading);
        assert!(state.details.is_none(), "details closes behind the player");

        reduce(
            &mut state,
            Intent::PlayerEvent(PlaybackEvent::Loaded {
                duration: Some(120.0),
            }),
        );
        assert_eq!(state.player.phase, PlayerPhase::Playing);

        reduce(
            &mut state,
            Intent::PlayerEvent(PlaybackEvent::Position {
                position: 42.0,
                duration: Some(120.0),
            }),
        );
        assert_eq!(state.player.position, 42.0);

        reduce(&mut state, Intent::PlayerEvent(PlaybackEvent::Ended));
        assert!(state.library.progress_for(&target.key()).is_some());
    }

    #[test]
    fn resume_position_skips_finished_items() {
        let mut state = AppState::default();
        let target = item("1");
        state.library.progress.push(PlaybackProgress {
            key: target.key(),
            item: target.clone(),
            position: 119.0,
            duration: 120.0,
            updated_at: 0,
            episode_id: None,
        });
        assert_eq!(resume_position(&state, &target), 0.0);

        state.library.progress[0].position = 30.0;
        assert_eq!(resume_position(&state, &target), 30.0);
    }

    #[test]
    fn repository_install_flow_updates_available_and_installed() {
        let mut state = AppState::default();
        let url = "https://example.test/repo.json".to_string();
        let repo = Repository {
            name: "Example".into(),
            description: String::new(),
            manifest_version: 1,
            extensions: vec![crate::domain::RepositoryExtension {
                provider_id: "example".into(),
                name: "Example".into(),
                version: "1.2.0".into(),
                description: String::new(),
                language: None,
                url: "https://example.test/example.wasm".into(),
                sha256: "00".repeat(32),
                icon_url: None,
                size_bytes: None,
            }],
            signature: None,
        };

        reduce(
            &mut state,
            Intent::RepositoryFetched {
                url: url.clone(),
                repository: Box::new(repo),
            },
        );
        assert_eq!(state.extensions.repositories.len(), 1);
        assert_eq!(state.extensions.available.len(), 1);

        let effects = reduce(
            &mut state,
            Intent::InstallExtension {
                repository: url.clone(),
                provider_id: "example".into(),
            },
        );
        assert!(matches!(effects[0], Effect::InstallExtension { .. }));

        reduce(
            &mut state,
            Intent::ExtensionInstalled(Box::new(InstalledExtension {
                provider_id: "example".into(),
                name: "Example".into(),
                version: "1.2.0".into(),
                repository: url,
                wasm_path: "C:/x/example.wasm".into(),
                sha256: "00".repeat(32),
                enabled: true,
                installed_at: 0,
            })),
        );
        assert!(state.extensions.is_installed("example"));
        assert!(!state.extensions.busy);
    }

    #[test]
    fn hydrate_restores_persisted_slices() {
        let mut state = AppState::default();
        let snapshot = PersistedSnapshot {
            settings: Settings {
                theme: crate::state::Theme::Light,
                ..Settings::default()
            },
            bookmarks: vec![Bookmark::new(item("7"), 1)],
            progress: vec![],
            repositories: vec![],
            extensions: vec![],
        };
        reduce(&mut state, Intent::Hydrate(Box::new(snapshot)));
        assert_eq!(state.settings.theme, crate::state::Theme::Light);
        assert_eq!(state.library.bookmarks.len(), 1);
    }

    #[test]
    fn failed_repository_keeps_its_reason_and_persists() {
        let mut state = AppState::default();
        let effects = reduce(
            &mut state,
            Intent::RepositoryFailed {
                url: "https://host.test/index.json".into(),
                error: "unsupported: Android repository".into(),
            },
        );
        let record = &state.extensions.repositories[0];
        assert_eq!(record.url, "https://host.test/index.json");
        assert!(record.last_error.is_some());
        assert!(effects
            .iter()
            .any(|effect| matches!(effect, Effect::PersistRepositories)));
    }

    fn builtin_view(id: &str) -> crate::state::BuiltinView {
        crate::state::BuiltinView {
            id: id.into(),
            name: id.into(),
            version: "1.0.0".into(),
            description: String::new(),
            origin: "https://example.test".into(),
            capabilities: "search".into(),
            enabled: true,
            testing: false,
            last_test: None,
        }
    }

    #[test]
    fn only_repositories_that_never_loaded_are_retried() {
        let mut state = AppState::default();
        state.extensions.repositories.push(RepositoryRecord {
            url: "https://host.test/ok.json".into(),
            name: "ok".into(),
            fetched_at: 10,
            extension_count: 3,
            last_error: None,
        });
        state.extensions.repositories.push(RepositoryRecord {
            url: "https://host.test/broken.json".into(),
            name: "broken".into(),
            fetched_at: 0,
            extension_count: 0,
            last_error: Some("unsupported".into()),
        });
        assert_eq!(
            state.extensions.repositories_to_refresh(),
            vec!["https://host.test/broken.json".to_string()]
        );
    }

    #[test]
    fn a_repeated_failure_is_not_announced_twice() {
        let mut state = AppState::default();
        let failure = || Intent::RepositoryFailed {
            url: "https://host.test/index.json".into(),
            error: "unsupported: Android repository".into(),
        };
        reduce(&mut state, failure());
        let after_first = state.toasts.len();
        reduce(&mut state, failure());
        assert_eq!(
            state.toasts.len(),
            after_first,
            "same error must stay quiet"
        );
        reduce(
            &mut state,
            Intent::RepositoryFailed {
                url: "https://host.test/index.json".into(),
                error: "network: timed out".into(),
            },
        );
        assert_eq!(state.toasts.len(), after_first + 1, "a new reason is news");
    }

    #[test]
    fn refresh_refetches_a_known_repository_only() {
        let mut state = AppState::default();
        state.extensions.repositories.push(RepositoryRecord {
            url: "https://host.test/index.json".into(),
            name: "known".into(),
            fetched_at: 0,
            extension_count: 0,
            last_error: Some("boom".into()),
        });
        let effects = reduce(
            &mut state,
            Intent::RefreshRepository("https://host.test/index.json".into()),
        );
        assert!(state.extensions.busy);
        assert!(matches!(
            effects.as_slice(),
            [Effect::FetchRepository { .. }]
        ));

        let mut unknown = AppState::default();
        let effects = reduce(
            &mut unknown,
            Intent::RefreshRepository("https://host.test/other.json".into()),
        );
        assert!(effects.is_empty());
    }

    #[test]
    fn removing_a_repository_keeps_installed_extensions() {
        let mut state = AppState::default();
        let url = "https://host.test/index.json".to_string();
        state.extensions.repositories.push(RepositoryRecord {
            url: url.clone(),
            name: "repo".into(),
            fetched_at: 1,
            extension_count: 1,
            last_error: None,
        });
        state
            .extensions
            .available
            .push(crate::state::AvailableExtension {
                repository: url.clone(),
                entry: crate::RepositoryExtension {
                    provider_id: "demo".into(),
                    name: "Demo".into(),
                    version: "1".into(),
                    description: String::new(),
                    language: None,
                    url: "https://host.test/demo.wasm".into(),
                    sha256: "ab".repeat(32),
                    icon_url: None,
                    size_bytes: None,
                },
            });
        state.extensions.installed.push(InstalledExtension {
            provider_id: "demo".into(),
            name: "Demo".into(),
            version: "1".into(),
            repository: url.clone(),
            wasm_path: "C:/x/demo.wasm".into(),
            sha256: "ab".repeat(32),
            enabled: true,
            installed_at: 0,
        });

        let effects = reduce(&mut state, Intent::RemoveRepository(url));
        assert!(state.extensions.repositories.is_empty());
        assert!(state.extensions.available.is_empty());
        assert_eq!(state.extensions.installed.len(), 1);
        assert!(effects
            .iter()
            .any(|effect| matches!(effect, Effect::PersistRepositories)));
    }

    #[test]
    fn disabling_a_builtin_persists_the_exclusion() {
        let mut state = AppState::default();
        state.extensions.builtins.push(builtin_view("builtin-demo"));
        let effects = reduce(
            &mut state,
            Intent::SetBuiltinEnabled {
                id: "builtin-demo".into(),
                enabled: false,
            },
        );
        assert!(!state.extensions.builtin("builtin-demo").unwrap().enabled);
        assert_eq!(state.settings.disabled_builtins, vec!["builtin-demo"]);
        assert!(effects
            .iter()
            .any(|effect| matches!(effect, Effect::PersistSettings)));
        assert!(effects
            .iter()
            .any(|effect| matches!(effect, Effect::ReloadProviders)));

        reduce(
            &mut state,
            Intent::SetBuiltinEnabled {
                id: "builtin-demo".into(),
                enabled: true,
            },
        );
        assert!(state.settings.disabled_builtins.is_empty());
        assert!(state.extensions.builtin("builtin-demo").unwrap().enabled);
    }

    #[test]
    fn a_builtin_test_runs_then_reports() {
        let mut state = AppState::default();
        state.extensions.builtins.push(builtin_view("builtin-demo"));

        let effects = reduce(&mut state, Intent::TestBuiltin("builtin-demo".into()));
        assert!(state.extensions.builtin("builtin-demo").unwrap().testing);
        assert!(matches!(effects.as_slice(), [Effect::TestBuiltin(_)]));

        reduce(
            &mut state,
            Intent::BuiltinTested {
                id: "builtin-demo".into(),
                ok: true,
                detail: "12 result(s) in 840 ms".into(),
            },
        );
        let view = state.extensions.builtin("builtin-demo").unwrap();
        assert!(!view.testing);
        let test = view.last_test.as_ref().expect("result stored");
        assert!(test.ok);
        assert_eq!(test.detail, "12 result(s) in 840 ms");
    }

    #[test]
    fn back_closes_player_then_details_then_route() {
        let mut state = AppState {
            route: Route::Browse,
            player: crate::state::PlayerState {
                visible: true,
                ..Default::default()
            },
            details: Some(DetailsState::new(item("1"))),
            ..AppState::default()
        };

        let effects = reduce(&mut state, Intent::Back);
        assert!(!state.player.visible);
        assert!(effects.iter().any(|e| matches!(e, Effect::PlayerStop)));
        assert!(state.details.is_some());

        reduce(&mut state, Intent::Back);
        assert!(state.details.is_none());
        assert_eq!(state.route, Route::Browse);

        reduce(&mut state, Intent::Back);
        assert_eq!(state.route, Route::Home);
    }
}
