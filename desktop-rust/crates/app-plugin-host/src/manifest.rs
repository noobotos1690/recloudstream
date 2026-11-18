//! Repository manifests: parsing, validation and trust.
//!
//! A repository is a JSON index the user adds by URL. Two independent checks run
//! before anything is installed:
//!
//! 1. **Structure** — every entry must have a provider id, an https URL and a
//!    64-character SHA-256 hash. An entry without a hash is refused, always.
//! 2. **Signature** (optional) — if the index carries an Ed25519 signature over
//!    its extension list, it must verify. An index *without* a signature is
//!    accepted, but the UI labels it untrusted and the hash still pins the
//!    binary, so a compromised index cannot silently swap a payload.

use app_core::{ProviderError, Repository, RepositoryExtension, RepositorySignature};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::Serialize;

/// Bump when the index format changes incompatibly.
pub const SUPPORTED_MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    /// No signature block: allowed, but shown as untrusted.
    Unsigned,
    /// Ed25519 signature verified against the embedded public key.
    Verified,
}

pub fn parse_repository(json: &str) -> Result<Repository, ProviderError> {
    // A stock CloudStream (Android) index is camelCase, has no `extensions`
    // array and points its `pluginLists` at Kotlin APKs. Detect it first so the
    // user reads why it cannot be used instead of a `missing field` serde error.
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(json) {
        let stock = value.get("pluginLists").is_some()
            || (value.get("manifestVersion").is_some() && value.get("extensions").is_none());
        if stock {
            return Err(ProviderError::Unsupported(
                "this is a CloudStream Android repository: its plugins are Kotlin APKs, while this desktop build runs WebAssembly extensions".to_string(),
            ));
        }
    }

    let repository: Repository = serde_json::from_str(json)
        .map_err(|err| ProviderError::Parse(format!("repository index: {err}")))?;
    validate_repository(&repository)?;
    Ok(repository)
}

pub fn validate_repository(repository: &Repository) -> Result<(), ProviderError> {
    if repository.manifest_version > SUPPORTED_MANIFEST_VERSION {
        return Err(ProviderError::Unsupported(format!(
            "repository needs manifest_version {}, this build understands {}",
            repository.manifest_version, SUPPORTED_MANIFEST_VERSION
        )));
    }
    for entry in &repository.extensions {
        validate_entry(entry)?;
    }
    Ok(())
}

pub fn validate_entry(entry: &RepositoryExtension) -> Result<(), ProviderError> {
    if entry.provider_id.trim().is_empty() {
        return Err(ProviderError::Parse(
            "extension is missing provider_id".into(),
        ));
    }
    if !is_allowed_id(&entry.provider_id) {
        return Err(ProviderError::Parse(format!(
            "provider_id `{}` contains unsupported characters",
            entry.provider_id
        )));
    }
    if !is_acceptable_url(&entry.url) {
        return Err(ProviderError::Parse(format!(
            "extension `{}` has a non-http url",
            entry.provider_id
        )));
    }
    if entry.sha256.len() != 64 || hex_decode(&entry.sha256).is_none() {
        return Err(ProviderError::Parse(format!(
            "extension `{}` has no valid SHA-256 hash",
            entry.provider_id
        )));
    }
    Ok(())
}

pub fn trust_level(repository: &Repository) -> Result<Trust, ProviderError> {
    match &repository.signature {
        None => Ok(Trust::Unsigned),
        Some(signature) => {
            verify(repository, signature)?;
            Ok(Trust::Verified)
        }
    }
}

/// Payload a repository signs: the canonical JSON of just the extension list, so
/// signing does not depend on the repository's display name.
pub fn signature_payload(repository: &Repository) -> Result<Vec<u8>, ProviderError> {
    #[derive(Serialize)]
    struct Payload<'a> {
        manifest_version: u32,
        extensions: &'a [RepositoryExtension],
    }
    serde_json::to_vec(&Payload {
        manifest_version: repository.manifest_version,
        extensions: &repository.extensions,
    })
    .map_err(|err| ProviderError::Parse(format!("cannot canonicalise index: {err}")))
}

fn verify(repository: &Repository, signature: &RepositorySignature) -> Result<(), ProviderError> {
    let key_bytes = hex_decode(&signature.public_key)
        .ok_or_else(|| ProviderError::Parse("signature public_key is not hex".into()))?;
    if key_bytes.len() != 32 {
        return Err(ProviderError::Parse(
            "signature public_key must be 32 bytes".into(),
        ));
    }
    let sig_bytes = hex_decode(&signature.signature)
        .ok_or_else(|| ProviderError::Parse("signature value is not hex".into()))?;
    if sig_bytes.len() != 64 {
        return Err(ProviderError::Parse("signature must be 64 bytes".into()));
    }

    let mut key = [0u8; 32];
    key.copy_from_slice(&key_bytes);
    let verifying_key = VerifyingKey::from_bytes(&key)
        .map_err(|err| ProviderError::Parse(format!("invalid public key: {err}")))?;

    let mut raw = [0u8; 64];
    raw.copy_from_slice(&sig_bytes);
    let signature = Signature::from_bytes(&raw);

    let payload = signature_payload(repository)?;
    verifying_key
        .verify_strict(&payload, &signature)
        .map_err(|_| ProviderError::Failed("repository signature did not verify".into()))
}

fn is_allowed_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Whether an entry URL may be handed to the downloader.
///
/// Both absolute and relative URLs are accepted, because `HttpRepository` takes
/// the index's own address as the base: a relative entry is joined onto that
/// https origin before any request is made, so it cannot escape the scheme
/// allow-list. Rejecting relative URLs here made `HttpRepository::resolve` dead
/// code and contradicted the ABI document, which tells repository authors that
/// `"url": "demo.wasm"` is valid next to the index.
///
/// A URL carrying *some other* scheme is still refused, as is a
/// protocol-relative one, so `file:`, `javascript:` and `//host/path` never
/// reach the network layer.
fn is_acceptable_url(url: &str) -> bool {
    let url = url.trim();
    if url.starts_with("https://") || url.starts_with("http://") {
        return true;
    }
    // Protocol-relative URLs would take the host from the *path*, so refuse them.
    if url.starts_with("//") {
        return false;
    }
    // Anything whose first path segment carries a colon is a scheme, not a path:
    // `file:`, `javascript:`, `ftp:`, and also a Windows drive letter. Testing
    // for "://" alone would let `javascript:alert(1)` through as "relative".
    if url.split('/').next().is_some_and(|head| head.contains(':')) {
        return false;
    }
    !url.is_empty()
}

pub fn hex_decode(input: &str) -> Option<Vec<u8>> {
    if input.len() % 2 != 0 {
        return None;
    }
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(input.len() / 2);
    let mut index = 0;
    while index < bytes.len() {
        let hi = hex_val(bytes[index])?;
        let lo = hex_val(bytes[index + 1])?;
        out.push((hi << 4) | lo);
        index += 2;
    }
    Some(out)
}

fn hex_val(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, hash: &str) -> RepositoryExtension {
        RepositoryExtension {
            provider_id: id.into(),
            name: id.into(),
            version: "1.0.0".into(),
            description: String::new(),
            language: None,
            url: "https://example.test/x.wasm".into(),
            sha256: hash.into(),
            icon_url: None,
            size_bytes: None,
        }
    }

    #[test]
    fn rejects_extension_without_a_hash() {
        let bad = entry("good", "");
        assert!(validate_entry(&bad).is_err());
        let ok = entry("good", &"ab".repeat(32));
        assert!(validate_entry(&ok).is_ok());
    }

    #[test]
    fn rejects_traversal_in_provider_id() {
        let bad = entry("../evil", &"ab".repeat(32));
        assert!(validate_entry(&bad).is_err());
    }

    #[test]
    fn unsigned_repository_is_allowed_but_labelled() {
        let repo = Repository {
            name: "Test".into(),
            description: String::new(),
            manifest_version: 1,
            extensions: vec![entry("a", &"cd".repeat(32))],
            signature: None,
        };
        assert_eq!(trust_level(&repo).unwrap(), Trust::Unsigned);
    }

    #[test]
    fn bad_signature_is_refused() {
        let repo = Repository {
            name: "Test".into(),
            description: String::new(),
            manifest_version: 1,
            extensions: vec![entry("a", &"cd".repeat(32))],
            signature: Some(RepositorySignature {
                public_key: "00".repeat(32),
                signature: "00".repeat(64),
            }),
        };
        assert!(trust_level(&repo).is_err());
    }

    #[test]
    fn future_manifest_version_is_refused() {
        let repo = Repository {
            name: "Future".into(),
            description: String::new(),
            manifest_version: 99,
            extensions: vec![],
            signature: None,
        };
        assert!(validate_repository(&repo).is_err());
    }

    #[test]
    fn stock_cloudstream_index_says_why_it_cannot_be_used() {
        let json = r#"{"name":"BDIX","iconUrl":"https://x/y.png","manifestVersion":1,
                       "pluginLists":["https://x/plugins.json"]}"#;
        let error = parse_repository(json).unwrap_err();
        assert!(matches!(error, ProviderError::Unsupported(_)), "{error:?}");
        assert!(error.message().contains("Android"), "{error}");
    }

    #[test]
    fn hex_round_trips() {
        let bytes = [0u8, 15, 16, 255];
        assert_eq!(hex_decode(&hex_encode(&bytes)).unwrap(), bytes.to_vec());
        assert!(hex_decode("zz").is_none());
    }

    /// A relative entry is resolved against the index's own https origin by
    /// `HttpRepository`, so it must survive validation instead of being rejected
    /// as a "non-http url".
    #[test]
    fn relative_entry_urls_are_accepted() {
        for url in [
            "ctgmovies.wasm",
            "./ctgmovies.wasm",
            "bin/a.wasm",
            "/root/a.wasm",
        ] {
            assert!(is_acceptable_url(url), "{url} should be accepted");
        }
    }

    #[test]
    fn non_http_schemes_are_still_rejected() {
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ftp://host/a.wasm",
            "//evil.test/a.wasm",
            "",
            "   ",
        ] {
            assert!(!is_acceptable_url(url), "{url} should be rejected");
        }
    }

    #[test]
    fn repository_with_a_relative_entry_parses() {
        let json = r#"{
            "name": "Relative repo",
            "manifest_version": 1,
            "extensions": [{
                "provider_id": "demo",
                "name": "Demo",
                "version": "1.0.0",
                "url": "demo.wasm",
                "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
            }]
        }"#;
        let repository = parse_repository(json).expect("relative url must validate");
        assert_eq!(repository.extensions[0].url, "demo.wasm");
    }
}
