//! Port traits: the seams between the pure core and the world.
//!
//! Every trait here is object-safe and `Send + Sync` where fan-out happens, so
//! the orchestrator can hold `Arc<dyn Provider>` for every enabled extension and
//! query them concurrently. Implementations live in their own crates; the core
//! never names a concrete backend.

use crate::domain::{
    AspectMode, HomeRow, MediaDetails, MediaItem, ProviderManifest, Quality, Repository,
    StreamSource, SubtitleTrack,
};
use std::fmt;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Everything a provider can fail with. The variants exist because the UI
/// treats them differently: `Network` is retryable, `Unsupported` hides a
/// button, `Sandbox` is a plugin bug worth surfacing verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    NotFound(String),
    Network(String),
    Unsupported(String),
    Parse(String),
    Sandbox(String),
    Failed(String),
}

impl ProviderError {
    pub fn unsupported(what: &str) -> Self {
        ProviderError::Unsupported(format!("provider does not implement `{what}`"))
    }

    pub fn message(&self) -> &str {
        match self {
            ProviderError::NotFound(m)
            | ProviderError::Network(m)
            | ProviderError::Unsupported(m)
            | ProviderError::Parse(m)
            | ProviderError::Sandbox(m)
            | ProviderError::Failed(m) => m,
        }
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            ProviderError::NotFound(_) => "not found",
            ProviderError::Network(_) => "network",
            ProviderError::Unsupported(_) => "unsupported",
            ProviderError::Parse(_) => "parse",
            ProviderError::Sandbox(_) => "plugin",
            ProviderError::Failed(_) => "failed",
        };
        write!(f, "{kind}: {}", self.message())
    }
}

impl std::error::Error for ProviderError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaybackError {
    EngineUnavailable(String),
    Load(String),
    Command(String),
    Render(String),
}

impl fmt::Display for PlaybackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlaybackError::EngineUnavailable(m) => write!(f, "engine unavailable: {m}"),
            PlaybackError::Load(m) => write!(f, "load failed: {m}"),
            PlaybackError::Command(m) => write!(f, "command failed: {m}"),
            PlaybackError::Render(m) => write!(f, "render failed: {m}"),
        }
    }
}

impl std::error::Error for PlaybackError {}

// ---------------------------------------------------------------------------
// Provider / extractor
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct ResolveOptions {
    /// `None` means "the provider decides" (a movie, or the first episode).
    pub episode_id: Option<String>,
    pub preferred_quality: Quality,
}

/// A content source. One per installed extension.
///
/// Implementations must be safe to call from several threads: the orchestrator
/// fans `search` out across providers concurrently. The WASM implementation
/// therefore serialises calls onto a single guest instance per provider, which
/// preserves the plugin's own invariants while keeping providers parallel with
/// each other.
pub trait Provider: Send + Sync {
    fn manifest(&self) -> ProviderManifest;

    fn home(&self) -> Result<Vec<HomeRow>, ProviderError> {
        Err(ProviderError::unsupported("home"))
    }

    fn search(&self, _query: &str) -> Result<Vec<MediaItem>, ProviderError> {
        Err(ProviderError::unsupported("search"))
    }

    fn details(&self, _id: &str) -> Result<MediaDetails, ProviderError> {
        Err(ProviderError::unsupported("details"))
    }

    fn resolve(
        &self,
        _id: &str,
        _options: &ResolveOptions,
    ) -> Result<Vec<StreamSource>, ProviderError> {
        Err(ProviderError::unsupported("resolve"))
    }
}

/// Turns a third-party embed/page URL into a playable source. Kept separate from
/// [`Provider`] because extraction is reusable across providers (many sites
/// share one CDN pattern) and because an extractor only needs one capability.
#[derive(Debug, Clone, Default)]
pub struct ExtractContext {
    pub headers: Vec<(String, String)>,
    /// Base URL of the page the link was found on; needed for relative URLs.
    pub referer: Option<String>,
}

pub trait Extractor: Send + Sync {
    fn name(&self) -> &str;
    /// Cheap prefix/host test so the host can pick an extractor without asking
    /// every one of them to try.
    fn supports(&self, url: &str) -> bool;
    fn extract(
        &self,
        url: &str,
        context: &ExtractContext,
    ) -> Result<Vec<StreamSource>, ProviderError>;
}

// ---------------------------------------------------------------------------
// Repository
// ---------------------------------------------------------------------------

/// Fetches a repository index and its binaries. Separate from [`Provider`] so
/// the extension manager does not need a WASM runtime to install extensions.
pub trait RepositorySource: Send + Sync {
    fn id(&self) -> &str;
    fn url(&self) -> &str;
    fn fetch(&self) -> Result<Repository, ProviderError>;
    fn fetch_binary(&self, url: &str) -> Result<Vec<u8>, ProviderError>;
}

// ---------------------------------------------------------------------------
// Networking
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HttpVerb {
    #[default]
    Get,
    Post,
    Head,
}

#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub url: String,
    pub verb: HttpVerb,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub timeout: Option<Duration>,
    /// Hard cap on the response body. Plugins are untrusted, so every host
    /// request carries one.
    pub max_bytes: usize,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            verb: HttpVerb::Get,
            headers: Vec::new(),
            body: None,
            timeout: None,
            max_bytes: 8 * 1024 * 1024,
        }
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

pub trait HttpClient: Send + Sync {
    fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ProviderError>;
}

// ---------------------------------------------------------------------------
// Playback
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PlaybackRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub subtitles: Vec<SubtitleTrack>,
    pub title: String,
    /// Resume point in seconds.
    pub start_position: f64,
    pub live: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackCommand {
    Play,
    Pause,
    TogglePause,
    SeekTo(f64),
    SeekRelative(f64),
    SetVolume(f64),
    SetMuted(bool),
    SetSpeed(f64),
    /// How the picture fills the surface (letterbox, crop, stretch).
    SetAspect(AspectMode),
    SetSubtitle(Option<String>),
    SetAudioTrack(Option<String>),
    Stop,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackEvent {
    Loaded {
        duration: Option<f64>,
    },
    Position {
        position: f64,
        duration: Option<f64>,
    },
    Paused(bool),
    Buffering(bool),
    Ended,
    Failed(String),
}

/// Destination for a decoded video frame.
///
/// The UI hands the engine the *already current* OpenGL framebuffer that egui
/// is drawing into, so the video is composited by the same context as the
/// interface: no child window, no z-order fights, no tearing during resize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderTarget {
    pub fbo: u32,
    pub width: u32,
    pub height: u32,
    pub flip_y: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EngineKind {
    /// No decoder available; the player still runs its timeline and UI so the
    /// rest of the app is usable (used on CI and before libmpv is installed).
    #[default]
    Null,
    LibMpv,
}

impl EngineKind {
    pub fn label(self) -> &'static str {
        match self {
            EngineKind::Null => "Timeline only",
            EngineKind::LibMpv => "libmpv",
        }
    }
}

/// Owns the decoder. Called from the UI thread for rendering and from the
/// runtime thread for transport commands; implementations keep the decoder on
/// its own thread and communicate by channel.
pub trait PlaybackEngine: Send {
    fn kind(&self) -> EngineKind;

    fn load(&mut self, request: PlaybackRequest) -> Result<(), PlaybackError>;

    fn command(&mut self, command: PlaybackCommand) -> Result<(), PlaybackError>;

    /// Non-blocking drain of everything that happened since the last call.
    fn poll(&mut self) -> Vec<PlaybackEvent>;

    /// True when a new video frame should be composited this frame.
    fn needs_render(&self) -> bool;

    /// Composite the current frame into `target`. Called on the UI thread with
    /// the OpenGL context current.
    fn render(&mut self, target: RenderTarget) -> Result<(), PlaybackError>;

    fn shutdown(&mut self);
}
