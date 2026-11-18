//! Provider-agnostic media vocabulary.
//!
//! Every type here is `serde`-serialisable on purpose: the same structs cross
//! the WASM plugin boundary, get written to disk by `app-storage` and are
//! rendered by `app-ui`. There is exactly one definition of "a media item", so
//! a plugin cannot drift from the host's expectations.

use serde::{Deserialize, Serialize};

/// What kind of media an entry represents. Drives badge rendering and whether
/// the details view offers an episode list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Movie,
    Series,
    Live,
    Audio,
    #[default]
    Other,
}

impl MediaKind {
    pub fn label(self) -> &'static str {
        match self {
            MediaKind::Movie => "Movie",
            MediaKind::Series => "Series",
            MediaKind::Live => "Live",
            MediaKind::Audio => "Audio",
            MediaKind::Other => "Video",
        }
    }
}

/// Stream quality hint. Ordering matters: [`Quality::rank`] is used to sort
/// resolved sources best-first when a provider does not express a preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    Uhd2160,
    Fhd1080,
    Hd720,
    Sd480,
    Low,
    #[default]
    Auto,
    Unknown,
}

impl Quality {
    pub fn label(self) -> &'static str {
        match self {
            Quality::Uhd2160 => "2160p",
            Quality::Fhd1080 => "1080p",
            Quality::Hd720 => "720p",
            Quality::Sd480 => "480p",
            Quality::Low => "Low",
            Quality::Auto => "Auto",
            Quality::Unknown => "Unknown",
        }
    }

    /// Higher is better. `Auto` ranks below every concrete quality because an
    /// adaptive source is only chosen when nothing better is offered.
    pub fn rank(self) -> u8 {
        match self {
            Quality::Uhd2160 => 60,
            Quality::Fhd1080 => 50,
            Quality::Hd720 => 40,
            Quality::Sd480 => 30,
            Quality::Low => 20,
            Quality::Unknown => 10,
            Quality::Auto => 5,
        }
    }
}

/// A single result tile. Deliberately small: search fans out over every enabled
/// provider, so this is the type that travels most.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaItem {
    /// Which extension produced this item.
    pub provider_id: String,
    /// Provider-scoped content identifier. Only meaningful to that provider.
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub kind: MediaKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    /// Short context line, e.g. "S2 E4" or "Live".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
}

impl MediaItem {
    pub fn new(
        provider_id: impl Into<String>,
        id: impl Into<String>,
        title: impl Into<String>,
    ) -> Self {
        Self {
            provider_id: provider_id.into(),
            id: id.into(),
            title: title.into(),
            kind: MediaKind::Other,
            poster: None,
            year: None,
            subtitle: None,
        }
    }

    /// Stable identity across sessions. Used as the bookmark key and the
    /// continue-watching key.
    pub fn key(&self) -> String {
        format!("{}::{}", self.provider_id, self.id)
    }

    pub fn with_poster(mut self, poster: impl Into<String>) -> Self {
        self.poster = Some(poster.into());
        self
    }

    pub fn with_kind(mut self, kind: MediaKind) -> Self {
        self.kind = kind;
        self
    }
}

/// One playable entry inside a series. Movies use a single synthetic episode
/// (`id == ""`), which keeps the details → resolve path uniform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Episode {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub season: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_secs: Option<u32>,
}

impl Episode {
    pub fn label(&self) -> String {
        match (self.season, self.number) {
            (Some(s), Some(n)) => format!("S{s} E{n} · {}", self.title),
            (None, Some(n)) => format!("E{n} · {}", self.title),
            _ => self.title.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaDetails {
    pub item: MediaItem,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backdrop: Option<String>,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub episodes: Vec<Episode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleTrack {
    pub url: String,
    #[serde(default)]
    pub language: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default)]
    pub default: bool,
}

/// A resolvable stream. `headers` are replayed by the playback engine, which is
/// how providers pass Referer/User-Agent/Origin to a CDN.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamSource {
    pub url: String,
    #[serde(default)]
    pub quality: Quality,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub subtitles: Vec<SubtitleTrack>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// `Some` when the source is a live stream with no seekable timeline.
    #[serde(default)]
    pub live: bool,
}

impl StreamSource {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            quality: Quality::Unknown,
            headers: Vec::new(),
            subtitles: Vec::new(),
            label: None,
            live: false,
        }
    }

    /// Human-readable description used in the source picker and in the player
    /// status line.
    pub fn display_label(&self) -> String {
        let mut parts = Vec::new();
        parts.push(
            self.label
                .clone()
                .unwrap_or_else(|| self.quality.label().to_string()),
        );
        if let Some(ext) = path_extension(&self.url) {
            parts.push(ext.to_ascii_uppercase());
        }
        if self.live {
            parts.push("LIVE".to_string());
        }
        parts.join(" · ")
    }
}

fn path_extension(url: &str) -> Option<&str> {
    let without_query = url.split(['?', '#']).next().unwrap_or(url);
    let last = without_query.rsplit('/').next()?;
    let (_, ext) = last.rsplit_once('.')?;
    if ext.is_empty() || ext.len() > 5 {
        None
    } else {
        Some(ext)
    }
}

/// One horizontal shelf on the home screen. Providers return these from `home`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomeRow {
    pub title: String,
    #[serde(default)]
    pub kind: RowKind,
    #[serde(default)]
    pub items: Vec<MediaItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowKind {
    #[default]
    Curated,
    Trending,
    Latest,
    ContinueWatching,
    Live,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub item: MediaItem,
    #[serde(default)]
    pub added_at: i64,
}

impl Bookmark {
    pub fn new(item: MediaItem, now: i64) -> Self {
        Self {
            item,
            added_at: now,
        }
    }
}

/// Resume point, written every few seconds while playback runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaybackProgress {
    pub key: String,
    pub item: MediaItem,
    pub position: f64,
    pub duration: f64,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode_id: Option<String>,
}

impl PlaybackProgress {
    /// Fraction watched in `0.0..=1.0`, or `0.0` when duration is unknown.
    pub fn fraction(&self) -> f32 {
        if self.duration <= 0.0 {
            0.0
        } else {
            (self.position / self.duration).clamp(0.0, 1.0) as f32
        }
    }
}

/// What a provider claims it can do. The UI hides tabs instead of showing
/// buttons that would fail; the orchestrator uses the same flags to decide
/// which plugins to fan a request out to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    #[serde(default)]
    pub home: bool,
    #[serde(default)]
    pub search: bool,
    #[serde(default)]
    pub details: bool,
    #[serde(default = "yes")]
    pub resolve: bool,
    #[serde(default)]
    pub live: bool,
}

fn yes() -> bool {
    true
}

impl Default for ProviderCapabilities {
    fn default() -> Self {
        Self {
            home: true,
            search: true,
            details: true,
            resolve: true,
            live: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub capabilities: ProviderCapabilities,
}

// ---------------------------------------------------------------------------
// Repository / extension distribution
// ---------------------------------------------------------------------------

/// A repository index, as fetched from a user-added URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repository {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Bumped by the repository when the index format changes.
    #[serde(default = "default_manifest_version")]
    pub manifest_version: u32,
    #[serde(default)]
    pub extensions: Vec<RepositoryExtension>,
    /// Optional Ed25519 signature over the canonical JSON of `extensions`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<RepositorySignature>,
}

fn default_manifest_version() -> u32 {
    1
}

/// Repositories the desktop build ships with, as `(display name, index URL)`.
///
/// They are pre-added on a profile that has never had a repository, so the
/// sources a build is meant to serve are one click away instead of a URL to
/// copy. Nothing is hidden: a built-in repository that cannot be used reports
/// its reason in the Extensions screen and stays visible.
pub const BUILTIN_REPOSITORIES: &[(&str, &str)] = &[(
    "Niloy's BDIX Repo",
    "https://raw.githubusercontent.com/Niloy-Sarker/niloy-cloudstream/master/repo.json",
)];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositorySignature {
    /// Hex-encoded 32-byte Ed25519 public key.
    pub public_key: String,
    /// Hex-encoded 64-byte signature.
    pub signature: String,
}

/// One installable extension listed by a repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryExtension {
    /// Must match the `id` the plugin reports in its manifest.
    pub provider_id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Absolute or repository-relative URL to the `.wasm` binary.
    pub url: String,
    /// Lowercase hex SHA-256 of the binary. Mandatory: installs are refused
    /// without it.
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

/// An extension present on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledExtension {
    pub provider_id: String,
    pub name: String,
    pub version: String,
    /// Repository the binary came from, for update checks.
    pub repository: String,
    pub wasm_path: String,
    pub sha256: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub installed_at: i64,
}

/// A user-added repository URL plus the outcome of the last fetch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryRecord {
    pub url: String,
    pub name: String,
    #[serde(default)]
    pub fetched_at: i64,
    #[serde(default)]
    pub extension_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// How the video surface fits the window. Kept as a preference because a
/// 4:3 IPTV feed and a 21:9 film need different answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AspectMode {
    #[default]
    Fit,
    Fill,
    Stretch,
    Original,
}

impl AspectMode {
    pub fn label(self) -> &'static str {
        match self {
            AspectMode::Fit => "Fit",
            AspectMode::Fill => "Fill",
            AspectMode::Stretch => "Stretch",
            AspectMode::Original => "Original",
        }
    }

    pub fn all() -> [AspectMode; 4] {
        [
            AspectMode::Fit,
            AspectMode::Fill,
            AspectMode::Stretch,
            AspectMode::Original,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_key_is_provider_scoped() {
        let a = MediaItem::new("p1", "42", "Same Title");
        let b = MediaItem::new("p2", "42", "Same Title");
        assert_ne!(a.key(), b.key());
        assert_eq!(a.key(), "p1::42");
    }

    #[test]
    fn quality_ranks_concrete_above_auto() {
        assert!(Quality::Fhd1080.rank() > Quality::Hd720.rank());
        assert!(Quality::Hd720.rank() > Quality::Auto.rank());
    }

    #[test]
    fn source_label_uses_extension_and_live_flag() {
        let mut s = StreamSource::new("https://cdn.example/a/b/playlist.m3u8?token=1");
        s.live = true;
        let label = s.display_label();
        assert!(label.contains("M3U8"), "{label}");
        assert!(label.contains("LIVE"), "{label}");
    }

    #[test]
    fn progress_fraction_is_clamped() {
        let p = PlaybackProgress {
            key: "k".into(),
            item: MediaItem::new("p", "1", "t"),
            position: 500.0,
            duration: 100.0,
            updated_at: 0,
            episode_id: None,
        };
        assert_eq!(p.fraction(), 1.0);
    }
}
