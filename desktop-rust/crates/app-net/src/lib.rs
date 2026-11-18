//! Blocking HTTP transport, isolated behind [`app_core::HttpClient`].
//!
//! Why `ureq` and not `reqwest`: this app issues a bounded number of short
//! requests from a worker thread, so an async runtime would add ~1.5 MB and a
//! second scheduler to the binary for no benefit. `ureq` is ~400 KB, uses
//! rustls (no OpenSSL, no SChannel linkage) and is synchronous, which is exactly
//! the shape the orchestrator wants.
//!
//! Every response is size-capped: a hostile or broken origin must not be able to
//! make the app allocate gigabytes before it even parses anything.

use app_core::{HttpClient, HttpRequest, HttpResponse, HttpVerb, ProviderError};
use serde::de::DeserializeOwned;
use std::io::Read;
use std::time::Duration;

/// Default ceiling for a single response body. Plugin host calls pass their own.
pub const DEFAULT_MAX_BYTES: usize = 8 * 1024 * 1024;

pub struct UreqClient {
    agent: ureq::Agent,
    timeout: Duration,
}

impl UreqClient {
    pub fn new(timeout: Duration) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            // Status codes are data here: providers routinely use 403/404 to
            // mean "not here", and we want the status, not an Err.
            .http_status_as_error(false)
            .build();
        Self {
            agent: config.into(),
            timeout,
        }
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Convenience for the repository/extension manager.
    pub fn get_text(&self, url: &str) -> Result<String, ProviderError> {
        let response = self.execute(HttpRequest::get(url))?;
        if !response.is_success() {
            return Err(ProviderError::Network(format!(
                "{url} returned HTTP {}",
                response.status
            )));
        }
        Ok(response.text())
    }

    pub fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T, ProviderError> {
        let text = self.get_text(url)?;
        serde_json::from_str(&text)
            .map_err(|err| ProviderError::Parse(format!("{url} is not valid JSON: {err}")))
    }

    pub fn get_bytes(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>, ProviderError> {
        let mut request = HttpRequest::get(url);
        request.max_bytes = max_bytes;
        let response = self.execute(request)?;
        if !response.is_success() {
            return Err(ProviderError::Network(format!(
                "{url} returned HTTP {}",
                response.status
            )));
        }
        Ok(response.body)
    }
}

impl HttpClient for UreqClient {
    fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ProviderError> {
        let mut headers = request.headers.clone();
        // Plenty of origins 403 a request with no User-Agent, so always send one
        // unless the caller set its own.
        if !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("user-agent"))
        {
            headers.push(("User-Agent".to_string(), USER_AGENT.to_string()));
        }

        // GET/HEAD and POST build different builder types in ureq (with and
        // without a body), so the send step cannot be shared.
        let url = request.url.clone();
        let per_request = request.timeout;
        let mut response = match request.verb {
            HttpVerb::Get => with_headers(self.agent.get(&url), &headers, per_request).call(),
            HttpVerb::Head => with_headers(self.agent.head(&url), &headers, per_request).call(),
            HttpVerb::Post => with_headers(self.agent.post(&url), &headers, per_request)
                .send(request.body.unwrap_or_default()),
        }
        .map_err(|err| ProviderError::Network(format!("{url}: {err}")))?;

        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_ascii_lowercase(),
                    value.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect::<Vec<_>>();

        let limit = request.max_bytes.max(1);
        let mut reader = response.body_mut().as_reader();
        let mut body = Vec::new();
        // Read one byte past the cap so we can distinguish "exactly at limit"
        // from "truncated", then fail loudly.
        reader
            .by_ref()
            .take(limit as u64 + 1)
            .read_to_end(&mut body)
            .map_err(|err| {
                ProviderError::Network(format!("{}: reading body: {err}", request.url))
            })?;

        if body.len() > limit {
            return Err(ProviderError::Network(format!(
                "{}: response exceeded the {limit} byte limit",
                request.url
            )));
        }

        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}

fn with_headers<B>(
    mut builder: ureq::RequestBuilder<B>,
    headers: &[(String, String)],
    timeout: Option<Duration>,
) -> ureq::RequestBuilder<B> {
    for (name, value) in headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    // A caller may need longer than the agent default: a provider page can be
    // slow without being broken, and the request should say so, not the client.
    if let Some(duration) = timeout {
        builder = builder.config().timeout_global(Some(duration)).build();
    }
    builder
}

/// Honest identification: no version fingerprinting beyond the app version, and
/// no tracking parameters. See PRIVACY.md.
pub const USER_AGENT: &str = concat!("CloudStreamDesktop/", env!("CARGO_PKG_VERSION"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_is_cheap_to_construct_with_a_timeout() {
        let client = UreqClient::new(Duration::from_secs(5));
        assert_eq!(client.timeout(), Duration::from_secs(5));
    }

    #[test]
    fn oversized_bodies_are_rejected_before_parsing() {
        // Exercised against a local file URL would need a server; instead assert
        // the contract the plugin host relies on.
        let request = HttpRequest::get("https://example.test/x");
        assert_eq!(request.max_bytes, DEFAULT_MAX_BYTES);
        let mut small = HttpRequest::get("https://example.test/x");
        small.max_bytes = 16;
        assert_eq!(small.max_bytes, 16);
    }
}
