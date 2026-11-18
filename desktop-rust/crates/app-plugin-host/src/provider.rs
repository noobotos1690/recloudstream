//! WASM-backed providers.
//!
//! Each installed extension becomes one [`WasmProvider`], which owns exactly one
//! [`extism::Plugin`] instance behind a mutex. That is the whole sandbox story:
//!
//! * no filesystem, no process, no raw sockets — the guest can only call the two
//!   host functions registered below;
//! * a fuel/time limit from the manifest stops runaway extractors;
//! * memory is capped at 64 pages (4 MiB), so a plugin cannot exhaust the host.
//!
//! Calls are serialised per provider but providers run in parallel, which is what
//! the orchestrator wants: one slow extension must not block the others.

use app_core::abi::{
    DetailsRequest, Envelope, HostHttpRequest, HostHttpResponse, ResolveRequest, SearchRequest,
};
use app_core::{
    HomeRow, HttpClient, HttpRequest, HttpVerb, MediaDetails, MediaItem, Provider, ProviderError,
    ProviderManifest, ResolveOptions, StreamSource,
};
use extism::{Manifest as ExtismManifest, Plugin, PluginBuilder, UserData, Wasm, PTR};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Guest memory ceiling in 64 KiB pages.
const GUEST_MEMORY_PAGES: u32 = 64;
/// Wall-clock ceiling for one guest call. Extractors do network I/O through the
/// host, so this has to be generous enough for a slow origin.
const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// Cap on what a guest may pull through the host HTTP function.
const HOST_HTTP_MAX_BYTES: usize = 8 * 1024 * 1024;

/// Services a guest is allowed to reach.
#[derive(Clone)]
pub struct HostContext {
    pub http: Arc<dyn HttpClient>,
    pub log: Arc<dyn Fn(String) + Send + Sync>,
}

// ---------------------------------------------------------------------------
// Host functions
// ---------------------------------------------------------------------------

extism::host_fn!(host_http_request(user: HostContext; payload: String) -> String {
    let shared = user.get()?;
    let context = shared
        .lock()
        .map_err(|_| extism::Error::msg("host context is unavailable"))?;
    let response = match serde_json::from_str::<HostHttpRequest>(&payload) {
        Ok(request) => perform_request(&context, request),
        Err(err) => HostHttpResponse {
            status: 0,
            headers: vec![],
            body: String::new(),
            error: Some(format!("invalid request payload: {err}")),
        },
    };
    serde_json::to_string(&response).map_err(|err| extism::Error::msg(err.to_string()))
});

extism::host_fn!(host_log(user: HostContext; line: String) -> String {
    let shared = user.get()?;
    let context = shared
        .lock()
        .map_err(|_| extism::Error::msg("host context is unavailable"))?;
    (context.log)(line);
    Ok(String::new())
});

fn perform_request(context: &HostContext, request: HostHttpRequest) -> HostHttpResponse {
    // Scheme allow-list: a plugin must not be able to touch the filesystem or
    // speak another protocol through the host.
    if !(request.url.starts_with("http://") || request.url.starts_with("https://")) {
        return HostHttpResponse {
            status: 0,
            headers: vec![],
            body: String::new(),
            error: Some("only http:// and https:// are allowed".to_string()),
        };
    }

    let verb = match request
        .method
        .as_deref()
        .unwrap_or("GET")
        .to_ascii_uppercase()
        .as_str()
    {
        "GET" => HttpVerb::Get,
        "POST" => HttpVerb::Post,
        "HEAD" => HttpVerb::Head,
        other => {
            return HostHttpResponse {
                status: 0,
                headers: vec![],
                body: String::new(),
                error: Some(format!("unsupported method {other}")),
            }
        }
    };

    let mut outgoing = HttpRequest::get(request.url);
    outgoing.verb = verb;
    outgoing.headers = request.headers;
    outgoing.max_bytes = HOST_HTTP_MAX_BYTES;
    outgoing.body = request.body.map(|body| body.into_bytes());

    match context.http.execute(outgoing) {
        Ok(response) => {
            let body = response.text();
            HostHttpResponse {
                status: response.status,
                headers: response.headers,
                body,
                error: None,
            }
        }
        Err(err) => HostHttpResponse {
            status: 0,
            headers: vec![],
            body: String::new(),
            error: Some(err.to_string()),
        },
    }
}

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

pub struct WasmProvider {
    manifest: ProviderManifest,
    plugin: Mutex<Plugin>,
}

impl WasmProvider {
    /// Instantiate a guest and ask it for its manifest. A guest that cannot
    /// describe itself is refused at load time rather than failing later.
    pub fn load(
        wasm_path: &str,
        http: Arc<dyn HttpClient>,
        log: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<Self, ProviderError> {
        let manifest = ExtismManifest::new([Wasm::file(wasm_path)])
            .with_timeout(CALL_TIMEOUT)
            .with_memory_max(GUEST_MEMORY_PAGES);

        let context = HostContext {
            http,
            log: Arc::clone(&log),
        };

        let mut plugin = PluginBuilder::new(manifest)
            .with_function(
                "host_http_request",
                [PTR],
                [PTR],
                UserData::new(context),
                host_http_request,
            )
            .with_function(
                "host_log",
                [PTR],
                [PTR],
                UserData::new(HostContext {
                    http: Arc::new(NoHttp),
                    log,
                }),
                host_log,
            )
            .build()
            .map_err(|err| ProviderError::Sandbox(format!("cannot instantiate guest: {err}")))?;

        let raw = plugin
            .call::<&str, Vec<u8>>("provider_manifest", "")
            .map_err(|err| ProviderError::Sandbox(format!("provider_manifest: {err}")))?;
        let manifest: ProviderManifest = serde_json::from_slice(&raw).map_err(|err| {
            ProviderError::Sandbox(format!("provider_manifest is not valid: {err}"))
        })?;
        if manifest.id.trim().is_empty() {
            return Err(ProviderError::Sandbox(
                "provider_manifest returned an empty id".to_string(),
            ));
        }

        Ok(Self {
            manifest,
            plugin: Mutex::new(plugin),
        })
    }

    pub fn manifest_ref(&self) -> &ProviderManifest {
        &self.manifest
    }

    fn call<T: DeserializeOwned, R: Serialize>(
        &self,
        function: &str,
        argument: &R,
    ) -> Result<T, ProviderError> {
        let payload = serde_json::to_vec(argument)
            .map_err(|err| ProviderError::Sandbox(format!("cannot encode {function}: {err}")))?;
        let mut plugin = self
            .plugin
            .lock()
            .map_err(|_| ProviderError::Sandbox("provider is unavailable".to_string()))?;
        let raw = plugin
            .call::<&[u8], Vec<u8>>(function, payload.as_slice())
            .map_err(|err| ProviderError::Sandbox(format!("{function}: {err}")))?;
        let envelope: Envelope<T> = serde_json::from_slice(&raw).map_err(|err| {
            ProviderError::Sandbox(format!("{function} returned bad JSON: {err}"))
        })?;
        envelope_to_result(function, envelope)
    }
}

fn envelope_to_result<T>(function: &str, envelope: Envelope<T>) -> Result<T, ProviderError> {
    if envelope.ok {
        return envelope
            .data
            .ok_or_else(|| ProviderError::Sandbox(format!("{function} returned no data")));
    }
    let message = envelope
        .error
        .unwrap_or_else(|| format!("{function} failed without a message"));
    Err(match envelope.kind.as_deref() {
        Some("not_found") => ProviderError::NotFound(message),
        Some("network") => ProviderError::Network(message),
        Some("unsupported") => ProviderError::Unsupported(message),
        Some("parse") => ProviderError::Parse(message),
        Some("sandbox") => ProviderError::Sandbox(message),
        _ => ProviderError::Failed(message),
    })
}

impl Provider for WasmProvider {
    fn manifest(&self) -> ProviderManifest {
        self.manifest.clone()
    }

    fn home(&self) -> Result<Vec<HomeRow>, ProviderError> {
        if !self.manifest.capabilities.home {
            return Err(ProviderError::unsupported("home"));
        }
        // The ABI passes an empty object rather than no argument, so a
        // future revision can add fields without breaking guests.
        self.call("home", &serde_json::json!({}))
    }

    fn search(&self, query: &str) -> Result<Vec<MediaItem>, ProviderError> {
        if !self.manifest.capabilities.search {
            return Err(ProviderError::unsupported("search"));
        }
        self.call(
            "search",
            &SearchRequest {
                query: query.to_string(),
            },
        )
    }

    fn details(&self, id: &str) -> Result<MediaDetails, ProviderError> {
        if !self.manifest.capabilities.details {
            return Err(ProviderError::unsupported("details"));
        }
        self.call("details", &DetailsRequest { id: id.to_string() })
    }

    fn resolve(
        &self,
        id: &str,
        options: &ResolveOptions,
    ) -> Result<Vec<StreamSource>, ProviderError> {
        if !self.manifest.capabilities.resolve {
            return Err(ProviderError::unsupported("resolve"));
        }
        self.call(
            "resolve",
            &ResolveRequest {
                id: id.to_string(),
                episode_id: options.episode_id.clone(),
                preferred_quality: options.preferred_quality,
            },
        )
    }
}

/// The log host function never performs I/O, so it gets a client that refuses
/// everything rather than sharing the real one.
struct NoHttp;

impl HttpClient for NoHttp {
    fn execute(&self, _request: HttpRequest) -> Result<app_core::HttpResponse, ProviderError> {
        Err(ProviderError::Sandbox(
            "this host function has no network access".to_string(),
        ))
    }
}

impl std::fmt::Debug for WasmProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmProvider")
            .field("provider_id", &self.manifest.id)
            .field("version", &self.manifest.version)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_error_maps_to_typed_error() {
        let envelope: Envelope<Vec<MediaItem>> = Envelope::error("not_found", "nope");
        let error = envelope_to_result("search", envelope).unwrap_err();
        assert!(matches!(error, ProviderError::NotFound(_)));
    }

    #[test]
    fn missing_data_is_a_sandbox_error() {
        let envelope: Envelope<Vec<MediaItem>> = Envelope::ok(vec![]);
        assert!(envelope_to_result("search", envelope).is_ok());
        let empty: Envelope<Vec<MediaItem>> = Envelope {
            ok: true,
            data: None,
            error: None,
            kind: None,
        };
        assert!(envelope_to_result("search", empty).is_err());
    }

    #[test]
    fn host_http_rejects_non_http_schemes() {
        let context = HostContext {
            http: Arc::new(NoHttp),
            log: Arc::new(|_| {}),
        };
        let response = perform_request(
            &context,
            HostHttpRequest {
                url: "file:///etc/passwd".to_string(),
                method: None,
                headers: vec![],
                body: None,
            },
        );
        assert_eq!(response.status, 0);
        assert!(response.error.unwrap().contains("only http"));
    }
}
