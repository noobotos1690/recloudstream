//! Loading installed extensions into live providers.

use crate::provider::WasmProvider;
use app_core::{HttpClient, InstalledExtension, Provider, ProviderError, ProviderManifest};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Log sink handed to guests; the app routes it to a file and the extensions
/// screen so a misbehaving plugin is diagnosable.
pub type LogSink = Arc<dyn Fn(String) + Send + Sync>;

#[derive(Default)]
pub struct ProviderRegistry {
    providers: Vec<Arc<dyn Provider>>,
    by_id: BTreeMap<String, Arc<dyn Provider>>,
    /// Extensions that failed to load, with the reason. Shown in the UI instead
    /// of being swallowed: a broken extension must be visible, not silent.
    pub failures: Vec<(String, String)>,
}

impl ProviderRegistry {
    pub fn empty() -> Self {
        Self::default()
    }

    /// Instantiate every enabled extension. Loading is per-extension and
    /// independent: one bad binary never prevents the others from working.
    pub fn load(
        extensions: &[InstalledExtension],
        http: Arc<dyn HttpClient>,
        log: LogSink,
    ) -> Self {
        let mut registry = Self::empty();
        for extension in extensions.iter().filter(|e| e.enabled) {
            match WasmProvider::load(&extension.wasm_path, Arc::clone(&http), Arc::clone(&log)) {
                Ok(provider) => {
                    let manifest = provider.manifest();
                    if manifest.id != extension.provider_id {
                        registry.failures.push((
                            extension.provider_id.clone(),
                            format!(
                                "extension reports id `{}` but was installed as `{}`",
                                manifest.id, extension.provider_id
                            ),
                        ));
                        continue;
                    }
                    let provider: Arc<dyn Provider> = Arc::new(provider);
                    registry
                        .by_id
                        .insert(manifest.id.clone(), Arc::clone(&provider));
                    registry.providers.push(provider);
                }
                Err(err) => registry
                    .failures
                    .push((extension.provider_id.clone(), err.to_string())),
            }
        }
        registry.providers.sort_by(|a, b| {
            a.manifest()
                .name
                .to_lowercase()
                .cmp(&b.manifest().name.to_lowercase())
        });
        registry
    }

    /// Add a provider compiled into the binary. Behaviourally identical to a
    /// loaded extension: same trait, same registry, same call sites.
    pub fn register(&mut self, provider: Arc<dyn Provider>) {
        let manifest = provider.manifest();
        self.failures.retain(|(id, _)| id != &manifest.id);
        self.providers
            .retain(|existing| existing.manifest().id != manifest.id);
        self.by_id
            .insert(manifest.id.clone(), Arc::clone(&provider));
        self.providers.push(provider);
        self.providers.sort_by(|a, b| {
            a.manifest()
                .name
                .to_lowercase()
                .cmp(&b.manifest().name.to_lowercase())
        });
    }

    pub fn manifests(&self) -> Vec<ProviderManifest> {
        self.providers.iter().map(|p| p.manifest()).collect()
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Provider>> {
        self.by_id.get(id).cloned()
    }

    pub fn all(&self) -> &[Arc<dyn Provider>] {
        &self.providers
    }

    pub fn len(&self) -> usize {
        self.providers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    /// Every provider that claims the given capability.
    pub fn with_capability(&self, test: fn(&ProviderManifest) -> bool) -> Vec<Arc<dyn Provider>> {
        self.providers
            .iter()
            .filter(|p| test(&p.manifest()))
            .cloned()
            .collect()
    }
}

/// Turn a provider error into something worth writing to the log.
pub fn describe(provider_id: &str, error: &ProviderError) -> String {
    format!("[{provider_id}] {error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_registry_reports_nothing() {
        let registry = ProviderRegistry::empty();
        assert!(registry.is_empty());
        assert!(registry.manifests().is_empty());
        assert!(registry.get("missing").is_none());
    }

    #[test]
    fn missing_binary_becomes_a_visible_failure() {
        struct NoHttp;
        impl HttpClient for NoHttp {
            fn execute(
                &self,
                _request: app_core::HttpRequest,
            ) -> Result<app_core::HttpResponse, ProviderError> {
                Err(ProviderError::Sandbox("no network".into()))
            }
        }

        let extension = InstalledExtension {
            provider_id: "demo".into(),
            name: "Demo".into(),
            version: "1.0.0".into(),
            repository: "https://host.test/index.json".into(),
            wasm_path: "C:/definitely/not/here.wasm".into(),
            sha256: "00".repeat(32),
            enabled: true,
            installed_at: 0,
        };

        let registry = ProviderRegistry::load(&[extension], Arc::new(NoHttp), Arc::new(|_| {}));
        assert_eq!(registry.len(), 0);
        assert_eq!(registry.failures.len(), 1);
        assert_eq!(registry.failures[0].0, "demo");
    }

    #[test]
    fn disabled_extensions_are_not_loaded() {
        struct NoHttp;
        impl HttpClient for NoHttp {
            fn execute(
                &self,
                _request: app_core::HttpRequest,
            ) -> Result<app_core::HttpResponse, ProviderError> {
                Err(ProviderError::Sandbox("no network".into()))
            }
        }

        let extension = InstalledExtension {
            provider_id: "demo".into(),
            name: "Demo".into(),
            version: "1.0.0".into(),
            repository: "https://host.test/index.json".into(),
            wasm_path: "C:/definitely/not/here.wasm".into(),
            sha256: "00".repeat(32),
            enabled: false,
            installed_at: 0,
        };

        let registry = ProviderRegistry::load(&[extension], Arc::new(NoHttp), Arc::new(|_| {}));
        assert!(registry.failures.is_empty());
    }
}
