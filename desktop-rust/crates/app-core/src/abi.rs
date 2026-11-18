//! The host ⇄ plugin wire format (ABI v1).
//!
//! Every guest export takes one UTF-8 JSON document and returns one:
//!
//! ```text
//! export provider_manifest()        -> Envelope<ProviderManifest>
//! export home()                     -> Envelope<Vec<HomeRow>>
//! export search(SearchRequest)      -> Envelope<Vec<MediaItem>>
//! export details(DetailsRequest)    -> Envelope<MediaDetails>
//! export resolve(ResolveRequest)    -> Envelope<Vec<StreamSource>>
//! ```
//!
//! Errors travel *inside* the envelope rather than as host failures, so a plugin
//! bug is reported with its own message instead of becoming a generic sandbox
//! error. `kind` uses the same vocabulary as [`crate::ProviderError`].

use crate::domain::{HomeRow, MediaDetails, MediaItem, ProviderManifest, Quality, StreamSource};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope<T> {
    pub ok: bool,
    // `Option` fields are optional in serde already; a `#[serde(default)]` here
    // would put a `T: Default` bound on the generic parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// One of `not_found`, `network`, `unsupported`, `parse`, `sandbox`, `failed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

impl<T> Envelope<T> {
    pub fn ok(data: T) -> Self {
        Self {
            ok: true,
            data: Some(data),
            error: None,
            kind: None,
        }
    }

    pub fn error(kind: &str, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(message.into()),
            kind: Some(kind.to_string()),
        }
    }
}

/// `search` argument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
}

/// `details` argument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetailsRequest {
    pub id: String,
}

/// `resolve` argument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveRequest {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode_id: Option<String>,
    #[serde(default)]
    pub preferred_quality: Quality,
}

/// Argument of the `http_request` host function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostHttpRequest {
    pub url: String,
    /// `GET` (default), `POST` or `HEAD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

/// Result of the `http_request` host function. The body is text: providers deal
/// in HTML and JSON, and refusing binary keeps the ABI free of a base64 layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostHttpResponse {
    /// `0` means the request never completed; `error` then holds the reason.
    pub status: u16,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Convenience alias so the guest SDK and the host agree on one type name.
pub type ManifestResponse = Envelope<ProviderManifest>;
pub type HomeResponse = Envelope<Vec<HomeRow>>;
pub type SearchResponse = Envelope<Vec<MediaItem>>;
pub type DetailsResponse = Envelope<MediaDetails>;
pub type ResolveResponse = Envelope<Vec<StreamSource>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_envelope_round_trips() {
        let envelope = Envelope::ok(vec![MediaItem::new("p", "1", "Title")]);
        let json = serde_json::to_string(&envelope).unwrap();
        let parsed: Envelope<Vec<MediaItem>> = serde_json::from_str(&json).unwrap();
        assert!(parsed.ok);
        assert_eq!(parsed.data.unwrap().len(), 1);
    }

    #[test]
    fn error_envelope_carries_a_kind() {
        let envelope: Envelope<Vec<MediaItem>> = Envelope::error("not_found", "nothing matched");
        let json = serde_json::to_string(&envelope).unwrap();
        assert!(json.contains("not_found"));
        assert!(json.contains("nothing matched"));
    }
}
