//! Side effects the reducer asks for but must not perform.
//!
//! The reducer returns `Vec<Effect>`; the runtime is the only thing that can
//! execute them. This is what keeps the state machine pure enough to unit test
//! and what stops a view from ever touching the network or the decoder.

use crate::domain::{MediaItem, RepositoryExtension};
use crate::ports::{PlaybackCommand, PlaybackRequest};

#[derive(Debug, Clone)]
pub enum Effect {
    /// Fetch every enabled provider's home rows.
    LoadHome,
    /// Fan a query out to the given providers.
    Search {
        query: String,
        provider_ids: Vec<String>,
    },
    LoadDetails {
        item: MediaItem,
    },
    ResolveSources {
        item: MediaItem,
        episode_id: Option<String>,
    },
    PlayerLoad {
        request: Box<PlaybackRequest>,
    },
    PlayerCommand(PlaybackCommand),
    PlayerStop,
    FetchRepository {
        url: String,
    },
    InstallExtension {
        repository: String,
        entry: Box<RepositoryExtension>,
    },
    DeleteExtensionFile {
        wasm_path: String,
    },
    /// Run a built-in provider's search once and report the outcome.
    TestBuiltin(String),
    /// Rebuild the provider registry after install/enable/disable/remove.
    ReloadProviders,
    PersistSettings,
    PersistBookmarks,
    PersistProgress,
    PersistRepositories,
    PersistExtensions,
}
