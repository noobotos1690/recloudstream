//! User intent: the complete vocabulary of things that can happen.
//!
//! Intents are split into two groups by convention:
//!
//! * *user* intents, produced by the UI (`Navigate`, `SearchSubmit`, …);
//! * *result* intents, produced by the runtime when a background job finishes
//!   (`HomeLoaded`, `SourcesResolved`, …).
//!
//! Both go through the same reducer, so there is exactly one place where state
//! changes and a background result cannot bypass validation.

use crate::domain::{
    AspectMode, HomeRow, InstalledExtension, MediaDetails, MediaItem, Repository, StreamSource,
};
use crate::ports::{EngineKind, PlaybackCommand, PlaybackEvent};
use crate::state::{PersistedSnapshot, Route, Settings, ToastKind};

#[derive(Debug, Clone)]
pub enum Intent {
    // -- navigation & shell -------------------------------------------------
    Navigate(Route),
    /// Leave whatever overlay is on top (details, then player).
    Back,
    /// Window metrics changed: resize, DPI change or maximise.
    Resize {
        width: f32,
        height: f32,
        dpi: f32,
    },
    Notice {
        kind: ToastKind,
        message: String,
    },
    DismissToast(usize),

    // -- catalog ------------------------------------------------------------
    RefreshHome,
    HomeLoaded(Vec<HomeRow>),
    HomeFailed(String),

    // -- search -------------------------------------------------------------
    SearchInput(String),
    SearchSubmit(String),
    /// One provider finished; results arrive per provider so the grid can fill
    /// in progressively instead of waiting for the slowest extension.
    SearchResults {
        provider_id: String,
        query: String,
        items: Vec<MediaItem>,
    },
    SearchFailed {
        provider_id: String,
        query: String,
        error: String,
    },

    // -- details & resolution ----------------------------------------------
    OpenDetails(Box<MediaItem>),
    CloseDetails,
    DetailsLoaded(Box<MediaDetails>),
    DetailsFailed(String),
    RequestSources {
        item: Box<MediaItem>,
        episode_id: Option<String>,
    },
    SourcesResolved {
        item: Box<MediaItem>,
        sources: Vec<StreamSource>,
    },
    SourcesFailed(String),

    // -- playback -----------------------------------------------------------
    PlaySource {
        source_index: usize,
    },
    ClosePlayer,
    PlayerCommand(PlaybackCommand),
    PlayerEvent(PlaybackEvent),
    SetAspect(AspectMode),
    SetVolume(f64),
    ToggleMute,
    SeekRelative(f64),
    SeekTo(f64),
    PlayerReady {
        engine: EngineKind,
        detail: String,
    },
    ProgressRecorded {
        item: Box<MediaItem>,
        episode_id: Option<String>,
        position: f64,
        duration: f64,
    },

    // -- bookmarks ----------------------------------------------------------
    ToggleBookmark(Box<MediaItem>),
    RemoveBookmark(String),

    // -- extensions ---------------------------------------------------------
    RepositoryDraftChanged(String),
    AddRepository(String),
    RepositoryFetched {
        url: String,
        repository: Box<Repository>,
    },
    RepositoryFailed {
        url: String,
        error: String,
    },
    /// Re-fetch an existing repository (the Refresh button).
    RefreshRepository(String),
    /// Forget a repository. Extensions already installed from it stay installed.
    RemoveRepository(String),
    /// Turn a built-in provider on or off.
    SetBuiltinEnabled {
        id: String,
        enabled: bool,
    },
    /// Run one online check so the user can see whether a provider still works.
    TestBuiltin(String),
    BuiltinTested {
        id: String,
        ok: bool,
        detail: String,
    },
    InstallExtension {
        repository: String,
        provider_id: String,
    },
    ExtensionInstalled(Box<InstalledExtension>),
    ExtensionFailed {
        provider_id: String,
        error: String,
    },
    SetExtensionEnabled {
        provider_id: String,
        enabled: bool,
    },
    RemoveExtension(String),
    /// A provider object became available (or was removed) after install; the
    /// registry itself lives in the runtime, this only refreshes the UI list.
    ProvidersRefreshed(Vec<crate::domain::ProviderManifest>),

    // -- settings & bootstrap ----------------------------------------------
    UpdateSettings(Box<Settings>),
    Hydrate(Box<PersistedSnapshot>),
}
