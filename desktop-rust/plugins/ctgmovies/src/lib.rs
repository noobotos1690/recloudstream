//! CTGMovies as an installable WebAssembly extension (ABI v1).
//!
//! The scraping logic is not reimplemented here. `app_providers::CtgMovies` is
//! the Rust port that ships compiled into the desktop binary, and this crate
//! reuses it verbatim, supplying a [`HttpClient`] that forwards to the Extism
//! host function instead of to `ureq`. One implementation of the markup rules
//! therefore backs both the built-in provider and the downloadable extension.
//!
//! Exports follow `docs/PLUGIN_ABI.md`:
//!
//! | export              | returns                        |
//! |---------------------|--------------------------------|
//! | `provider_manifest` | the bare `ProviderManifest`    |
//! | `home`              | `Envelope<Vec<HomeRow>>`       |
//! | `search`            | `Envelope<Vec<MediaItem>>`     |
//! | `details`           | `Envelope<MediaDetails>`       |
//! | `resolve`           | `Envelope<Vec<StreamSource>>`  |
//!
//! `provider_manifest` is the one export the host parses *without* an envelope,
//! because a plugin that cannot describe itself is rejected before any call.

use app_core::abi::{
    DetailsRequest, Envelope, HostHttpRequest, HostHttpResponse, ResolveRequest, SearchRequest,
};
use app_core::{
    HttpClient, HttpRequest, HttpResponse, HttpVerb, MediaItem, Provider, ProviderError,
    ResolveOptions,
};
use app_providers::CtgMovies;
use extism_pdk::*;
use std::sync::Arc;

/// Identity of the *installable* extension.
///
/// `app_providers::CtgMovies` reports `builtin-ctgmovies`, the id of the copy
/// compiled into the binary. An extension must not reuse it: the registry keys
/// providers by id and replaces on collision, so installing this module would
/// silently shadow the built-in (or be shadowed by it) instead of running as its
/// own provider. The id also has to match every `MediaItem` the module returns,
/// so the outputs are re-tagged below.
const GUEST_ID: &str = "ctgmovies";

#[host_fn]
extern "ExtismHost" {
    fn host_http_request(payload: String) -> String;
}

// ---------------------------------------------------------------------------
// Host networking
// ---------------------------------------------------------------------------

/// The only network the guest has. Every call is a JSON round-trip to the host,
/// which applies the scheme allow-list, the 8 MiB cap and the timeout.
struct GuestHttp;

impl HttpClient for GuestHttp {
    fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ProviderError> {
        let method = match request.verb {
            HttpVerb::Get => "GET",
            HttpVerb::Post => "POST",
            HttpVerb::Head => "HEAD",
        };
        let payload = HostHttpRequest {
            url: request.url,
            method: Some(method.to_string()),
            headers: request.headers,
            // The ABI body is text; providers here only ever POST form data.
            body: request
                .body
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
        };
        let encoded = serde_json::to_string(&payload)
            .map_err(|err| ProviderError::Failed(format!("cannot encode request: {err}")))?;

        // Safety: `host_http_request` is registered by the host for every plugin
        // instance; the ABI guarantees it exists for the lifetime of the guest.
        let raw = unsafe { host_http_request(encoded) }
            .map_err(|err| ProviderError::Network(format!("host_http_request: {err}")))?;

        let reply: HostHttpResponse = serde_json::from_str(&raw)
            .map_err(|err| ProviderError::Failed(format!("host reply is not JSON: {err}")))?;
        if let Some(error) = reply.error {
            return Err(ProviderError::Network(error));
        }
        Ok(HttpResponse {
            status: reply.status,
            headers: reply.headers,
            body: reply.body.into_bytes(),
        })
    }
}

fn provider() -> CtgMovies {
    CtgMovies::new(Arc::new(GuestHttp))
}

// ---------------------------------------------------------------------------
// Re-tagging
// ---------------------------------------------------------------------------

/// Point an item at this extension rather than at the built-in it was ported
/// from. The host uses `provider_id` to key media, so it must be `GUEST_ID`.
fn retag_item(item: &mut MediaItem) {
    item.provider_id = GUEST_ID.to_string();
}

fn retag_all(items: &mut [MediaItem]) {
    for item in items.iter_mut() {
        retag_item(item);
    }
}

// ---------------------------------------------------------------------------
// Envelope helpers
// ---------------------------------------------------------------------------

/// `kind` uses the same vocabulary as `ProviderError`, which is what lets the
/// host decide whether an error is retryable (`network`), hides a button
/// (`unsupported`) or shows an empty state (`not_found`).
fn kind_of(error: &ProviderError) -> &'static str {
    match error {
        ProviderError::NotFound(_) => "not_found",
        ProviderError::Network(_) => "network",
        ProviderError::Unsupported(_) => "unsupported",
        ProviderError::Parse(_) => "parse",
        ProviderError::Sandbox(_) => "sandbox",
        ProviderError::Failed(_) => "failed",
    }
}

fn reply<T: serde::Serialize>(result: Result<T, ProviderError>) -> String {
    let envelope = match result {
        Ok(data) => Envelope::ok(data),
        Err(error) => Envelope::error(kind_of(&error), error.message()),
    };
    serde_json::to_string(&envelope).unwrap_or_else(|err| {
        // Serialising our own envelope should not fail, but returning a trap here
        // would cost the user the actual reason, so fall back to a plain error.
        format!(
            r#"{{"ok":false,"kind":"failed","error":"cannot encode reply: {}"}}"#,
            err.to_string().replace('"', "'")
        )
    })
}

// ---------------------------------------------------------------------------
// Exports
// ---------------------------------------------------------------------------

/// Called once at load time and parsed without an envelope: a plugin that
/// cannot describe itself is refused before anything else runs.
#[plugin_fn]
pub fn provider_manifest(_input: String) -> FnResult<String> {
    let mut manifest = provider().manifest();
    manifest.id = GUEST_ID.to_string();
    manifest.description =
        "Films, series and anime, with direct file links for playback. Installable copy of the \
         built-in CTGMovies provider."
            .to_string();
    Ok(serde_json::to_string(&manifest)?)
}

#[plugin_fn]
pub fn home(_input: String) -> FnResult<String> {
    let result = provider().home().map(|mut rows| {
        for row in rows.iter_mut() {
            retag_all(&mut row.items);
        }
        rows
    });
    Ok(reply(result))
}

#[plugin_fn]
pub fn search(input: String) -> FnResult<String> {
    let request: SearchRequest = serde_json::from_str(&input)?;
    let result = provider().search(&request.query).map(|mut items| {
        retag_all(&mut items);
        items
    });
    Ok(reply(result))
}

#[plugin_fn]
pub fn details(input: String) -> FnResult<String> {
    let request: DetailsRequest = serde_json::from_str(&input)?;
    let result = provider().details(&request.id).map(|mut details| {
        retag_item(&mut details.item);
        details
    });
    Ok(reply(result))
}

#[plugin_fn]
pub fn resolve(input: String) -> FnResult<String> {
    let request: ResolveRequest = serde_json::from_str(&input)?;
    let options = ResolveOptions {
        episode_id: request.episode_id.clone(),
        preferred_quality: request.preferred_quality,
    };
    Ok(reply(provider().resolve(&request.id, &options)))
}
