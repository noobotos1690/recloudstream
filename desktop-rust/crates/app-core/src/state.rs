//! The single immutable-ish application state.
//!
//! Conventions that keep this tractable:
//!
//! * One struct, no getters, no interior mutability. Views read it; only the
//!   reducer writes it.
//! * Anything that can block (network, decode, disk) is *not* here. State holds
//!   only what a frame needs to draw: values, not handles.
//! * Persisted slices are plain `Vec`s that `app-storage` serialises verbatim.

use crate::domain::{
    AspectMode, Bookmark, HomeRow, InstalledExtension, MediaDetails, MediaItem, PlaybackProgress,
    ProviderManifest, Quality, RepositoryExtension, RepositoryRecord, StreamSource, SubtitleTrack,
};
use crate::layout::Breakpoint;
use crate::ports::{EngineKind, PlaybackCommand};
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Route {
    Home,
    Browse,
    Search,
    Bookmarks,
    Extensions,
    Settings,
}

impl Route {
    pub const ALL: [Route; 6] = [
        Route::Home,
        Route::Browse,
        Route::Search,
        Route::Bookmarks,
        Route::Extensions,
        Route::Settings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Route::Home => "Home",
            Route::Browse => "Providers",
            Route::Search => "Search",
            Route::Bookmarks => "Bookmarks",
            Route::Extensions => "Extensions",
            Route::Settings => "Settings",
        }
    }
}

// ---------------------------------------------------------------------------
// Preferences
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Dark,
    Light,
}

impl Theme {
    pub fn label(self) -> &'static str {
        match self {
            Theme::System => "System",
            Theme::Dark => "Dark",
            Theme::Light => "Light",
        }
    }

    pub fn all() -> [Theme; 3] {
        [Theme::System, Theme::Dark, Theme::Light]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: Theme,
    pub aspect: AspectMode,
    pub hardware_decoding: bool,
    pub cache_limit_mb: u32,
    pub preferred_quality: Quality,
    pub network_timeout_secs: u64,
    /// Trim animation for reduced-motion users and low-end machines.
    pub reduce_motion: bool,
    /// Debug aid and accessibility escape hatch: pin the layout class.
    pub force_layout: Option<Breakpoint>,
    pub resume_playback: bool,
    /// Built-in provider ids the user switched off. Storing the exclusions
    /// rather than the inclusions means a provider added in a new build is on
    /// by default, and every existing profile keeps working.
    pub disabled_builtins: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            aspect: AspectMode::Fit,
            hardware_decoding: true,
            cache_limit_mb: 1024,
            preferred_quality: Quality::Auto,
            network_timeout_secs: 25,
            reduce_motion: false,
            force_layout: None,
            resume_playback: true,
            disabled_builtins: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Player
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Default)]
pub enum PlayerPhase {
    #[default]
    Idle,
    Loading,
    Playing,
    Paused,
    Ended,
    Failed(String),
}

impl PlayerPhase {
    pub fn is_active(self) -> bool {
        matches!(self, PlayerPhase::Playing | PlayerPhase::Paused)
    }

    pub fn label(&self) -> &'static str {
        match self {
            PlayerPhase::Idle => "Idle",
            PlayerPhase::Loading => "Loading",
            PlayerPhase::Playing => "Playing",
            PlayerPhase::Paused => "Paused",
            PlayerPhase::Ended => "Ended",
            PlayerPhase::Failed(_) => "Error",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlayerState {
    pub visible: bool,
    pub item: Option<MediaItem>,
    pub episode_id: Option<String>,
    pub sources: Vec<StreamSource>,
    pub active_source: Option<usize>,
    pub phase: PlayerPhase,
    pub position: f64,
    pub duration: f64,
    pub engine: EngineKind,
    pub aspect: AspectMode,
    pub volume: f64,
    pub muted: bool,
    pub buffering: bool,
    pub subtitle: Option<String>,
    pub subtitles: Vec<SubtitleTrack>,
    /// Resume offset applied on load, so progress starts where the user left it.
    pub resume_from: f64,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            visible: false,
            item: None,
            episode_id: None,
            sources: Vec::new(),
            active_source: None,
            phase: PlayerPhase::Idle,
            position: 0.0,
            duration: 0.0,
            engine: EngineKind::Null,
            aspect: AspectMode::Fit,
            volume: 1.0,
            muted: false,
            buffering: false,
            subtitle: None,
            subtitles: Vec::new(),
            resume_from: 0.0,
        }
    }
}

impl PlayerState {
    pub fn active_stream(&self) -> Option<&StreamSource> {
        self.active_source.and_then(|i| self.sources.get(i))
    }

    pub fn progress_fraction(&self) -> f32 {
        if self.duration <= 0.0 {
            0.0
        } else {
            (self.position / self.duration).clamp(0.0, 1.0) as f32
        }
    }

    pub fn reset(&mut self) {
        *self = PlayerState {
            engine: self.engine,
            aspect: self.aspect,
            volume: self.volume,
            muted: self.muted,
            ..PlayerState::default()
        };
    }

    /// Transport commands that make sense right now; the player overlay renders
    /// these and the keyboard handler maps to the same set.
    pub fn transport_hint(&self) -> Vec<PlaybackCommand> {
        match self.phase {
            PlayerPhase::Playing => {
                vec![PlaybackCommand::Pause, PlaybackCommand::SeekRelative(-10.0)]
            }
            PlayerPhase::Paused => vec![PlaybackCommand::Play],
            _ => vec![PlaybackCommand::Play],
        }
    }
}

// ---------------------------------------------------------------------------
// Screen-local state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct LibraryState {
    pub rows: Vec<HomeRow>,
    pub loading: bool,
    pub loaded_once: bool,
    pub bookmarks: Vec<Bookmark>,
    pub progress: Vec<PlaybackProgress>,
}

impl LibraryState {
    pub fn is_bookmarked(&self, key: &str) -> bool {
        self.bookmarks.iter().any(|b| b.item.key() == key)
    }

    pub fn progress_for(&self, key: &str) -> Option<&PlaybackProgress> {
        self.progress.iter().find(|p| p.key == key)
    }

    /// Continue-watching, newest first. Trimmed by the caller for display.
    pub fn continue_watching(&self, limit: usize) -> Vec<&PlaybackProgress> {
        let mut items: Vec<&PlaybackProgress> = self
            .progress
            .iter()
            .filter(|p| p.duration > 0.0 && p.fraction() > 0.02 && p.fraction() < 0.95)
            .collect();
        items.sort_by_key(|entry| std::cmp::Reverse(entry.updated_at));
        items.truncate(limit);
        items
    }
}

#[derive(Debug, Clone, Default)]
pub struct BrowseState {
    pub providers: Vec<ProviderManifest>,
    pub selected: Option<String>,
}

impl BrowseState {
    pub fn provider(&self, id: &str) -> Option<&ProviderManifest> {
        self.providers.iter().find(|p| p.id == id)
    }
}

#[derive(Debug, Clone, Default)]
pub struct SearchState {
    pub query: String,
    /// Query of the in-flight result set; used to discard stale provider replies.
    pub active_query: String,
    pub results: Vec<MediaItem>,
    pub pending: Vec<String>,
    pub results_by_provider: Vec<(String, usize)>,
    pub failures: Vec<(String, String)>,
}

impl SearchState {
    pub fn is_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    pub fn clear_results(&mut self) {
        self.results.clear();
        self.pending.clear();
        self.results_by_provider.clear();
        self.failures.clear();
    }
}

#[derive(Debug, Clone)]
pub struct DetailsState {
    pub item: MediaItem,
    pub details: Option<MediaDetails>,
    pub loading: bool,
    pub resolving: bool,
    pub sources: Vec<StreamSource>,
    pub error: Option<String>,
}

impl DetailsState {
    pub fn new(item: MediaItem) -> Self {
        Self {
            item,
            details: None,
            loading: true,
            resolving: false,
            sources: Vec::new(),
            error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableExtension {
    pub repository: String,
    pub entry: RepositoryExtension,
}

#[derive(Debug, Clone, Default)]
pub struct ExtensionsState {
    pub repositories: Vec<RepositoryRecord>,
    pub available: Vec<AvailableExtension>,
    pub installed: Vec<InstalledExtension>,
    /// Providers compiled into the binary. Filled in by the runtime from
    /// `app-providers`, because the core must not know concrete providers.
    pub builtins: Vec<BuiltinView>,
    /// Text in the "add repository" box.
    pub draft_repository: String,
    pub busy: bool,
}

/// One built-in provider as the Extensions screen shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinView {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// Site the provider talks to.
    pub origin: String,
    /// What it implements, as a display string ("home · search · details").
    pub capabilities: String,
    pub enabled: bool,
    /// An online check is running right now.
    pub testing: bool,
    pub last_test: Option<BuiltinTest>,
}

/// Outcome of the last online check, shown next to the Test button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinTest {
    pub ok: bool,
    pub detail: String,
}

impl ExtensionsState {
    /// Repositories that have never loaded, and so are worth retrying at start.
    pub fn repositories_to_refresh(&self) -> Vec<String> {
        self.repositories
            .iter()
            .filter(|record| record.last_error.is_some())
            .map(|record| record.url.clone())
            .collect()
    }

    pub fn builtin(&self, id: &str) -> Option<&BuiltinView> {
        self.builtins.iter().find(|builtin| builtin.id == id)
    }

    pub fn builtin_mut(&mut self, id: &str) -> Option<&mut BuiltinView> {
        self.builtins.iter_mut().find(|builtin| builtin.id == id)
    }

    pub fn is_installed(&self, provider_id: &str) -> bool {
        self.installed.iter().any(|e| e.provider_id == provider_id)
    }

    pub fn available_for(&self, repository: &str) -> Vec<&AvailableExtension> {
        self.available
            .iter()
            .filter(|a| a.repository == repository)
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub kind: ToastKind,
    pub message: String,
    pub created_at: Instant,
}

impl Toast {
    pub fn new(kind: ToastKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            created_at: Instant::now(),
        }
    }
}

/// What the host actually managed to initialise, shown on the settings screen so
/// a missing DLL is a visible fact rather than a mystery crash.
#[derive(Debug, Clone)]
pub struct BackendStatus {
    pub engine: EngineKind,
    pub engine_detail: String,
    pub wasm_runtime: bool,
    pub data_dir: String,
    pub extension_dir: String,
}

impl Default for BackendStatus {
    fn default() -> Self {
        Self {
            engine: EngineKind::Null,
            engine_detail: "not initialised".to_string(),
            wasm_runtime: false,
            data_dir: String::new(),
            extension_dir: String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Root state
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct AppState {
    pub route: Route,
    pub breakpoint: Breakpoint,
    /// Logical points, already DPI-adjusted by the windowing layer.
    pub window_width: f32,
    pub window_height: f32,
    /// Pixels per point, kept for diagnostics and for the video-surface maths.
    pub dpi_scale: f32,
    pub library: LibraryState,
    pub browse: BrowseState,
    pub search: SearchState,
    pub details: Option<DetailsState>,
    pub player: PlayerState,
    pub extensions: ExtensionsState,
    pub settings: Settings,
    pub toasts: Vec<Toast>,
    /// Bumped whenever a repaint is required without user input.
    pub needs_repaint: bool,
    pub backend: BackendStatus,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            route: Route::Home,
            breakpoint: Breakpoint::Medium,
            window_width: 1280.0,
            window_height: 800.0,
            dpi_scale: 1.0,
            library: LibraryState::default(),
            browse: BrowseState::default(),
            search: SearchState::default(),
            details: None,
            player: PlayerState::default(),
            extensions: ExtensionsState::default(),
            settings: Settings::default(),
            toasts: Vec::new(),
            needs_repaint: false,
            backend: BackendStatus::default(),
        }
    }
}

impl AppState {
    pub fn push_toast(&mut self, kind: ToastKind, message: impl Into<String>) {
        self.toasts.push(Toast::new(kind, message));
        // Keep the queue bounded; three visible toasts is already a lot.
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
    }

    pub fn expire_toasts(&mut self, max_age: std::time::Duration) {
        let now = Instant::now();
        self.toasts
            .retain(|t| now.duration_since(t.created_at) < max_age);
    }

    /// The item whose details/player context is current, if any.
    pub fn focused_item(&self) -> Option<&MediaItem> {
        self.player
            .item
            .as_ref()
            .or_else(|| self.details.as_ref().map(|d| &d.item))
    }

    pub fn layout(&self) -> crate::layout::LayoutMetrics {
        // Width/height are already logical points, so DPI is 1.0 here; the scale
        // factor is applied by the windowing layer before we ever see a size.
        crate::layout::LayoutMetrics::compute(
            self.window_width,
            self.window_height,
            1.0,
            self.settings.force_layout,
        )
    }
}

/// Everything that survives a restart. `app-storage` owns the file formats;
/// this is the in-memory shape they deserialize into.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PersistedSnapshot {
    pub settings: Settings,
    pub bookmarks: Vec<Bookmark>,
    pub progress: Vec<PlaybackProgress>,
    pub repositories: Vec<RepositoryRecord>,
    pub extensions: Vec<InstalledExtension>,
}
