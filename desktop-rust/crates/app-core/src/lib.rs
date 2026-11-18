//! Core of the desktop media centre: domain model, MVI state machine and the
//! port traits every other crate implements.
//!
//! Layering rule enforced by the crate graph:
//!
//! ```text
//! app-ui ─┐
//! app-playback ─┤
//! app-plugin-host ─┼─> app-core        (pure: serde only)
//! app-storage ─┤
//! app-net ─┘
//! ```
//!
//! Nothing here knows about egui, libmpv, wasm or the filesystem, so the whole
//! application state machine can be exercised in tests without a GUI.

pub mod abi;
pub mod domain;
pub mod effect;
pub mod intent;
pub mod layout;
pub mod ports;
pub mod reducer;
pub mod state;

pub use domain::{
    AspectMode, Bookmark, Episode, HomeRow, InstalledExtension, MediaDetails, MediaItem, MediaKind,
    PlaybackProgress, ProviderCapabilities, ProviderManifest, Quality, Repository,
    RepositoryExtension, RepositoryRecord, RepositorySignature, RowKind, StreamSource,
    SubtitleTrack, BUILTIN_REPOSITORIES,
};
pub use effect::Effect;
pub use intent::Intent;
pub use layout::{Breakpoint, LayoutMetrics};
pub use ports::{
    EngineKind, ExtractContext, Extractor, HttpClient, HttpRequest, HttpResponse, HttpVerb,
    PlaybackCommand, PlaybackEngine, PlaybackError, PlaybackEvent, PlaybackRequest, Provider,
    ProviderError, RenderTarget, RepositorySource, ResolveOptions,
};
pub use reducer::reduce;
pub use state::{
    AppState, BuiltinTest, BuiltinView, PersistedSnapshot, PlayerPhase, PlayerState, Route,
    Settings, Theme, Toast, ToastKind,
};
