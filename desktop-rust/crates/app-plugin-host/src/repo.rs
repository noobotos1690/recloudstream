//! Downloading repository indexes and extension binaries.

use crate::manifest::parse_repository;
use app_core::{HttpClient, HttpRequest, ProviderError, Repository, RepositorySource};
use std::sync::Arc;

/// Index files are small; a 4 MB cap is generous and stops a hostile origin from
/// streaming forever.
const MAX_INDEX_BYTES: usize = 4 * 1024 * 1024;
/// Extensions are hand-written logic. 16 MB is already far larger than any
/// sensible provider; anything bigger is a red flag.
pub const MAX_EXTENSION_BYTES: usize = 16 * 1024 * 1024;

pub struct HttpRepository {
    url: String,
    http: Arc<dyn HttpClient>,
}

impl HttpRepository {
    pub fn new(url: impl Into<String>, http: Arc<dyn HttpClient>) -> Self {
        Self {
            url: url.into(),
            http,
        }
    }

    /// Resolve a possibly relative entry URL against the index URL.
    pub fn resolve(&self, maybe_relative: &str) -> String {
        if maybe_relative.starts_with("http://") || maybe_relative.starts_with("https://") {
            return maybe_relative.to_string();
        }
        match self.url.rsplit_once('/') {
            Some((base, _)) => format!("{base}/{}", maybe_relative.trim_start_matches('/')),
            None => maybe_relative.to_string(),
        }
    }
}

impl RepositorySource for HttpRepository {
    fn id(&self) -> &str {
        &self.url
    }

    fn url(&self) -> &str {
        &self.url
    }

    fn fetch(&self) -> Result<Repository, ProviderError> {
        let mut request = HttpRequest::get(self.url.clone());
        request.max_bytes = MAX_INDEX_BYTES;
        request
            .headers
            .push(("Accept".into(), "application/json".into()));
        let response = self.http.execute(request)?;
        if !response.is_success() {
            return Err(ProviderError::Network(format!(
                "{} returned HTTP {}",
                self.url, response.status
            )));
        }
        parse_repository(&response.text())
    }

    fn fetch_binary(&self, url: &str) -> Result<Vec<u8>, ProviderError> {
        let mut request = HttpRequest::get(self.resolve(url));
        request.max_bytes = MAX_EXTENSION_BYTES;
        let response = self.http.execute(request)?;
        if !response.is_success() {
            return Err(ProviderError::Network(format!(
                "{url} returned HTTP {}",
                response.status
            )));
        }
        Ok(response.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_core::{HttpResponse, HttpVerb};
    use std::sync::Mutex;

    struct StubHttp {
        body: String,
        seen: Mutex<Vec<String>>,
    }

    impl HttpClient for StubHttp {
        fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ProviderError> {
            assert_eq!(request.verb, HttpVerb::Get);
            self.seen.lock().unwrap().push(request.url);
            Ok(HttpResponse {
                status: 200,
                headers: vec![],
                body: self.body.clone().into_bytes(),
            })
        }
    }

    fn stub(body: &str) -> Arc<StubHttp> {
        Arc::new(StubHttp {
            body: body.to_string(),
            seen: Mutex::new(Vec::new()),
        })
    }

    #[test]
    fn relative_urls_resolve_against_the_index() {
        let http = stub("{}");
        let repo = HttpRepository::new("https://host.test/deep/index.json", http);
        assert_eq!(
            repo.resolve("ext/a.wasm"),
            "https://host.test/deep/ext/a.wasm"
        );
        assert_eq!(
            repo.resolve("/root/a.wasm"),
            "https://host.test/deep/root/a.wasm"
        );
        assert_eq!(
            repo.resolve("https://cdn.test/a.wasm"),
            "https://cdn.test/a.wasm"
        );
    }

    #[test]
    fn fetch_parses_and_validates_the_index() {
        let body = r#"{
            "name": "Test repo",
            "manifest_version": 1,
            "extensions": [{
                "provider_id": "demo",
                "name": "Demo",
                "version": "1.0.0",
                "url": "https://host.test/demo.wasm",
                "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
            }]
        }"#;
        let http = stub(body);
        let repo = HttpRepository::new("https://host.test/index.json", http.clone());
        let parsed = repo.fetch().expect("index parses");
        assert_eq!(parsed.extensions.len(), 1);
        assert_eq!(http.seen.lock().unwrap()[0], "https://host.test/index.json");
    }

    #[test]
    fn invalid_index_is_a_parse_error_not_a_panic() {
        let http = stub("{\"name\":\"x\",\"extensions\":[{\"provider_id\":\"a\"}]}");
        let repo = HttpRepository::new("https://host.test/index.json", http);
        let error = repo.fetch().unwrap_err();
        // Missing required fields (name/version/url/sha256) must be reported.
        assert!(matches!(error, ProviderError::Parse(_)), "{error:?}");
    }

    /// The index actually shipped in `releases/<version>/repo.json` must parse.
    ///
    /// The first revision of that file copied Android's `fileHash` verbatim,
    /// i.e. `sha256-<hex>`, but the desktop schema stores the bare 64-character
    /// hex digest, so every entry was rejected as "no valid SHA-256 hash" while
    /// the file still looked correct. Compiling the shipped bytes into the test
    /// keeps the two from drifting apart again.
    #[test]
    fn shipped_release_index_parses_and_validates() {
        let shipped = include_str!("../../../releases/v1.3.0/repo.json");
        let repository = crate::manifest::parse_repository(shipped)
            .expect("releases/v1.3.0/repo.json must parse and validate");
        assert_eq!(repository.manifest_version, 1);
        assert!(!repository.name.trim().is_empty());
    }

    /// A prefixed hash is the exact mistake described above; pin the rejection.
    #[test]
    fn sha256_prefix_from_android_format_is_rejected() {
        let body = r#"{
            "name": "Android-shaped",
            "manifest_version": 1,
            "extensions": [{
                "provider_id": "demo",
                "name": "Demo",
                "version": "1.0.0",
                "url": "https://host.test/demo.wasm",
                "sha256": "sha256-650b4cdb442f9ad31bf9fdce8b1c29bd6842ffdf0774d0ee5bf8a63fbb560cc7"
            }]
        }"#;
        let error = crate::manifest::parse_repository(body).unwrap_err();
        assert!(matches!(error, ProviderError::Parse(_)), "{error:?}");
    }
}
