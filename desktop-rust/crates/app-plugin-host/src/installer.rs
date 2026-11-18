//! Installing and removing extension binaries.
//!
//! The order is deliberate: download → verify hash → *then* write. Nothing is
//! written to the extensions directory before its bytes match the hash the index
//! promised, so a corrupted or tampered payload never becomes loadable.

use crate::repo::HttpRepository;
use app_core::{
    HttpClient, InstalledExtension, ProviderError, RepositoryExtension, RepositorySource,
};
use app_storage::Store;
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub struct ExtensionInstaller {
    http: Arc<dyn HttpClient>,
    store: Store,
}

impl ExtensionInstaller {
    pub fn new(http: Arc<dyn HttpClient>, store: Store) -> Self {
        Self { http, store }
    }

    pub fn install(
        &self,
        repository_url: &str,
        entry: &RepositoryExtension,
    ) -> Result<InstalledExtension, ProviderError> {
        let repo = HttpRepository::new(repository_url.to_string(), Arc::clone(&self.http));
        let binary = repo.fetch_binary(&entry.url)?;

        let actual = hex_sha256(&binary);
        if !actual.eq_ignore_ascii_case(&entry.sha256) {
            return Err(ProviderError::Failed(format!(
                "hash mismatch for {}: repository said {}, received {}",
                entry.provider_id, entry.sha256, actual
            )));
        }

        let path = self
            .store
            .write_extension(&entry.provider_id, &entry.sha256, &binary)
            .map_err(|err| ProviderError::Failed(err.to_string()))?;

        Ok(InstalledExtension {
            provider_id: entry.provider_id.clone(),
            name: entry.name.clone(),
            version: entry.version.clone(),
            repository: repository_url.to_string(),
            wasm_path: path.to_string_lossy().into_owned(),
            sha256: actual,
            enabled: true,
            installed_at: unix_now(),
        })
    }

    /// Remove the binary. A missing file is not an error: the registry entry is
    /// the thing that matters and it is removed regardless.
    pub fn remove(&self, wasm_path: &str) -> Result<(), ProviderError> {
        let path = std::path::Path::new(wasm_path);
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(ProviderError::Failed(format!(
                "cannot remove {}: {err}",
                path.display()
            ))),
        }
    }
}

pub fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_core::{HttpRequest, HttpResponse};
    use std::sync::Mutex;

    struct Stub {
        bytes: Vec<u8>,
        seen: Mutex<usize>,
    }

    impl HttpClient for Stub {
        fn execute(&self, _request: HttpRequest) -> Result<HttpResponse, ProviderError> {
            *self.seen.lock().unwrap() += 1;
            Ok(HttpResponse {
                status: 200,
                headers: vec![],
                body: self.bytes.clone(),
            })
        }
    }

    fn store() -> Store {
        let mut root = std::env::temp_dir();
        root.push(format!("cs-installer-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        Store::open(root).expect("store")
    }

    fn entry_for(hash: &str) -> RepositoryExtension {
        RepositoryExtension {
            provider_id: "demo".into(),
            name: "Demo".into(),
            version: "1.0.0".into(),
            description: String::new(),
            language: None,
            url: "https://host.test/demo.wasm".into(),
            sha256: hash.to_string(),
            icon_url: None,
            size_bytes: None,
        }
    }

    #[test]
    fn installs_when_hash_matches() {
        let payload = b"\0asm\x01\0\0\0".to_vec();
        let hash = hex_sha256(&payload);
        let http = Arc::new(Stub {
            bytes: payload.clone(),
            seen: Mutex::new(0),
        });
        let installer = ExtensionInstaller::new(http, store());
        let installed = installer
            .install("https://host.test/index.json", &entry_for(&hash))
            .expect("install");
        assert_eq!(installed.sha256, hash);
        assert!(std::path::Path::new(&installed.wasm_path).exists());
    }

    #[test]
    fn refuses_to_write_when_hash_differs() {
        let payload = b"tampered".to_vec();
        let http = Arc::new(Stub {
            bytes: payload.clone(),
            seen: Mutex::new(0),
        });
        let installer = ExtensionInstaller::new(http, store());
        let error = installer
            .install("https://host.test/index.json", &entry_for(&"ab".repeat(32)))
            .unwrap_err();
        assert!(matches!(error, ProviderError::Failed(_)));
        assert!(error.to_string().contains("hash mismatch"));
    }

    #[test]
    fn remove_tolerates_a_missing_file() {
        let http = Arc::new(Stub {
            bytes: vec![],
            seen: Mutex::new(0),
        });
        let installer = ExtensionInstaller::new(http, store());
        assert!(installer.remove("C:/definitely/not/here.wasm").is_ok());
    }
}
