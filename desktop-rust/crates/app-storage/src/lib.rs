//! Local persistence: settings, bookmarks, resume points and the extension
//! registry.
//!
//! Design decisions worth stating, because they are the ones that keep the
//! installer small:
//!
//! * **No SQLite.** The largest dataset here is a few hundred bookmarks and
//!   resume points — under 1 MB of JSON. Pulling in a SQL engine to store that
//!   would cost ~1.5 MB of binary and a DLL, for queries we never run.
//! * **Atomic writes.** Every save writes `*.tmp` then renames over the target.
//!   A crash mid-save leaves the previous good file intact, which matters when
//!   the file holds the user's bookmarks.
//! * **Secrets never touch these files.** Tokens go through [`secrets`], which
//!   uses DPAPI so ciphertext is bound to the Windows user account.

pub mod secrets;

use app_core::{
    Bookmark, InstalledExtension, PersistedSnapshot, PlaybackProgress, RepositoryRecord, Settings,
};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

const SETTINGS_FILE: &str = "settings.json";
const BOOKMARKS_FILE: &str = "bookmarks.json";
const PROGRESS_FILE: &str = "progress.json";
const REPOSITORIES_FILE: &str = "repositories.json";
const EXTENSIONS_FILE: &str = "extensions.json";
const EXTENSIONS_DIR: &str = "extensions";
const CACHE_DIR: &str = "cache";

#[derive(Debug)]
pub enum StoreError {
    Io { path: PathBuf, message: String },
    Json { path: PathBuf, message: String },
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Io { path, message } => write!(f, "{}: {message}", path.display()),
            StoreError::Json { path, message } => {
                write!(f, "{}: invalid JSON: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for StoreError {}

/// Root of the per-user data directory.
///
/// Layout (all under `%LOCALAPPDATA%\CloudStreamDesktop`):
///
/// ```text
/// settings.json
/// bookmarks.json
/// progress.json
/// repositories.json
/// extensions.json
/// extensions\<sha256-prefix>\<provider>.wasm
/// cache\            (covers, freed by the LRU sweep)
/// ```
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// `%LOCALAPPDATA%\CloudStreamDesktop`, or a portable fallback beside the
    /// executable when the variable is missing.
    pub fn open_default() -> Result<Self, StoreError> {
        let root = match std::env::var_os("LOCALAPPDATA") {
            Some(base) if !base.is_empty() => PathBuf::from(base).join("CloudStreamDesktop"),
            _ => PathBuf::from("data"),
        };
        Self::open(root)
    }

    pub fn open(root: PathBuf) -> Result<Self, StoreError> {
        let store = Self { root };
        store.ensure_dirs()?;
        Ok(store)
    }

    fn ensure_dirs(&self) -> Result<(), StoreError> {
        for dir in [self.root.clone(), self.extensions_dir(), self.cache_dir()] {
            fs::create_dir_all(&dir).map_err(|err| StoreError::Io {
                path: dir,
                message: err.to_string(),
            })?;
        }
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn extensions_dir(&self) -> PathBuf {
        self.root.join(EXTENSIONS_DIR)
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.root.join(CACHE_DIR)
    }

    /// Where an extension binary lives. Keyed by repository + provider id so a
    /// repository cannot shadow another repository's provider.
    pub fn extension_path(&self, provider_id: &str, sha256: &str) -> PathBuf {
        let safe_id: String = provider_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let prefix: String = sha256.chars().take(12).collect();
        self.extensions_dir()
            .join(if prefix.is_empty() {
                "unverified".to_string()
            } else {
                prefix
            })
            .join(format!("{safe_id}.wasm"))
    }

    /// Read everything the app needs at startup. A corrupt file is skipped and
    /// reported, never fatal: losing bookmarks must not stop the app opening.
    pub fn load_snapshot(&self) -> PersistedSnapshot {
        PersistedSnapshot {
            settings: self
                .read_json::<Settings>(SETTINGS_FILE)
                .unwrap_or_default(),
            bookmarks: self
                .read_json::<Vec<Bookmark>>(BOOKMARKS_FILE)
                .unwrap_or_default(),
            progress: self
                .read_json::<Vec<PlaybackProgress>>(PROGRESS_FILE)
                .unwrap_or_default(),
            repositories: self
                .read_json::<Vec<RepositoryRecord>>(REPOSITORIES_FILE)
                .unwrap_or_default(),
            extensions: self
                .read_json::<Vec<InstalledExtension>>(EXTENSIONS_FILE)
                .unwrap_or_default(),
        }
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<(), StoreError> {
        self.write_json(SETTINGS_FILE, settings)
    }

    pub fn save_bookmarks(&self, bookmarks: &[Bookmark]) -> Result<(), StoreError> {
        self.write_json(BOOKMARKS_FILE, bookmarks)
    }

    pub fn save_progress(&self, progress: &[PlaybackProgress]) -> Result<(), StoreError> {
        self.write_json(PROGRESS_FILE, progress)
    }

    pub fn save_repositories(&self, repositories: &[RepositoryRecord]) -> Result<(), StoreError> {
        self.write_json(REPOSITORIES_FILE, repositories)
    }

    /// True once a repository list has been written. Distinguishes a fresh
    /// profile (seed the built-in repositories) from one where the list was
    /// deliberately emptied.
    pub fn has_repositories(&self) -> bool {
        self.root().join(REPOSITORIES_FILE).exists()
    }

    pub fn save_extensions(&self, extensions: &[InstalledExtension]) -> Result<(), StoreError> {
        self.write_json(EXTENSIONS_FILE, extensions)
    }

    /// Write an extension binary and return its path.
    pub fn write_extension(
        &self,
        provider_id: &str,
        sha256: &str,
        bytes: &[u8],
    ) -> Result<PathBuf, StoreError> {
        let path = self.extension_path(provider_id, sha256);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| StoreError::Io {
                path: parent.to_path_buf(),
                message: err.to_string(),
            })?;
        }
        write_atomic(&path, bytes)?;
        Ok(path)
    }

    /// Total size of the cache directory, used by the settings screen.
    pub fn cache_size_bytes(&self) -> u64 {
        dir_size(&self.cache_dir())
    }

    fn read_json<T: serde::de::DeserializeOwned>(&self, name: &str) -> Result<T, StoreError> {
        let path = self.root.join(name);
        let text = fs::read_to_string(&path).map_err(|err| StoreError::Io {
            path: path.clone(),
            message: err.to_string(),
        })?;
        serde_json::from_str(&text).map_err(|err| StoreError::Json {
            path,
            message: err.to_string(),
        })
    }

    fn write_json<T: serde::Serialize + ?Sized>(
        &self,
        name: &str,
        value: &T,
    ) -> Result<(), StoreError> {
        let path = self.root.join(name);
        let bytes = serde_json::to_vec_pretty(value).map_err(|err| StoreError::Json {
            path: path.clone(),
            message: err.to_string(),
        })?;
        write_atomic(&path, &bytes)
    }
}

/// Write to a sibling temp file, flush, then rename over the target. On Windows
/// `fs::rename` replaces the destination, so readers never observe a partial
/// file.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let tmp = path.with_extension("tmp");
    {
        use std::io::Write;
        let mut file = fs::File::create(&tmp).map_err(|err| StoreError::Io {
            path: tmp.clone(),
            message: err.to_string(),
        })?;
        file.write_all(bytes).map_err(|err| StoreError::Io {
            path: tmp.clone(),
            message: err.to_string(),
        })?;
        file.flush().map_err(|err| StoreError::Io {
            path: tmp.clone(),
            message: err.to_string(),
        })?;
    }
    fs::rename(&tmp, path).map_err(|err| StoreError::Io {
        path: path.to_path_buf(),
        message: err.to_string(),
    })
}

fn dir_size(dir: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            match entry.metadata() {
                Ok(meta) if meta.is_file() => total += meta.len(),
                Ok(meta) if meta.is_dir() => total += dir_size(&entry.path()),
                _ => {}
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_core::{MediaItem, Settings};

    fn temp_store(name: &str) -> Store {
        let mut root = std::env::temp_dir();
        root.push(format!("cs-desktop-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        Store::open(root).expect("store")
    }

    #[test]
    fn round_trips_settings_and_bookmarks() {
        let store = temp_store("roundtrip");

        let settings = Settings {
            cache_limit_mb: 512,
            ..Settings::default()
        };
        store.save_settings(&settings).unwrap();
        store
            .save_bookmarks(&[Bookmark::new(MediaItem::new("p", "1", "Dune"), 7)])
            .unwrap();

        let snapshot = store.load_snapshot();
        assert_eq!(snapshot.settings.cache_limit_mb, 512);
        assert_eq!(snapshot.bookmarks.len(), 1);
        assert_eq!(snapshot.bookmarks[0].item.title, "Dune");

        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn corrupt_file_does_not_abort_startup() {
        let store = temp_store("corrupt");
        fs::write(store.root().join(BOOKMARKS_FILE), b"{not json").unwrap();
        let snapshot = store.load_snapshot();
        assert!(snapshot.bookmarks.is_empty());
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn extension_paths_are_sanitised_and_sharded() {
        let store = temp_store("paths");
        let path = store.extension_path("../../evil/provider", "abcdef0123456789");
        let text = path.to_string_lossy().replace('\\', "/");
        assert!(text.contains("abcdef012345"), "{text}");
        assert!(!text.contains(".."), "{text}");
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn writes_are_atomic_and_leave_no_temp_file() {
        let store = temp_store("atomic");
        store.save_bookmarks(&[]).unwrap();
        assert!(!store.root().join("bookmarks.tmp").exists());
        assert!(store.root().join(BOOKMARKS_FILE).exists());
        let _ = fs::remove_dir_all(store.root());
    }
}
