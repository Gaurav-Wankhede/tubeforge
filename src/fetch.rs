//! Fetch layer (LLD §5): RSS feeds, oEmbed, YouTube Data API v3 + quota
//! ledger. Shared HTTP client and retry plumbing live here.
//!
//! Policy (LLD §5, §6): 15s timeout, 3× exponential-backoff retries on
//! 500/429/timeout, ETag caching for RSS, `search.list` NEVER used.

pub mod api;
pub mod oembed;
pub mod quota;
pub mod rss;
pub mod ytdlp;

use std::future::Future;
use std::time::Duration;

use crate::error::Source;
use crate::error::{storage_err, TubeforgeError};

/// YouTube autocomplete suggestions for a query, from Google's public
/// suggestqueries endpoint (`client=youtube` returns real YouTube search
/// suggestions — the same source VidIQ's "Related Keywords" uses).
/// Keyless: no API key, no quota. Returns the suggested query strings.
pub async fn youtube_suggestions(clients: &FetchClients, q: &str) -> Vec<String> {
    let url = format!(
        "https://suggestqueries.google.com/complete/search?client=youtube&gs_ri=youtube&ds=yt&hl=en&q={}",
        urlencoding(q)
    );
    let Ok(resp) = clients.http.get(&url).send().await else {
        return Vec::new();
    };
    let Ok(body) = resp.text().await else {
        return Vec::new();
    };
    // Shape: `window.google.ac.h(["q",[["sug",0,[512]],...],...])`.
    let Some(start) = body.find("[[") else {
        return Vec::new();
    };
    let Some(end) = body[start..].find("]]") else {
        return Vec::new();
    };
    let chunk = &body[start + 2..start + end];
    let mut out = Vec::new();
    for part in chunk.split("],") {
        let trimmed = part.trim_start_matches('[').trim_start_matches('"');
        let sug = trimmed.split('"').next().unwrap_or("");
        if !sug.is_empty() {
            out.push(sug.to_string());
        }
    }
    out
}

/// Percent-encode a query string for the suggest endpoint.
fn urlencoding(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub const DEFAULT_RSS_BASE: &str = "https://www.youtube.com";
pub const DEFAULT_OEMBED_BASE: &str = "https://www.youtube.com";
pub const DEFAULT_API_BASE: &str = "https://www.googleapis.com/youtube/v3";

/// Request timeout (LLD §5.1).
pub const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

/// Retries after the initial attempt (LLD §5.1: "3× exponential backoff").
pub const MAX_RETRIES: u32 = 3;

/// Shared HTTP client + endpoint bases. Bases are overridable so tests can
/// point the whole fetch layer at a wiremock server.
pub struct FetchClients {
    pub http: reqwest::Client,
    pub rss_base: String,
    pub oembed_base: String,
    pub api_base: String,
}

impl FetchClients {
    /// Production client: real YouTube endpoints, 15s timeout.
    pub fn new() -> Result<Self, TubeforgeError> {
        Self::with_bases(
            DEFAULT_RSS_BASE,
            DEFAULT_OEMBED_BASE,
            DEFAULT_API_BASE,
            HTTP_TIMEOUT,
        )
    }

    /// Client with all bases pointing at `base` (wiremock in tests).
    pub fn for_test(base: &str, timeout: Duration) -> Result<Self, TubeforgeError> {
        Self::with_bases(base, base, base, timeout)
    }

    pub fn with_bases(
        rss_base: &str,
        oembed_base: &str,
        api_base: &str,
        timeout: Duration,
    ) -> Result<Self, TubeforgeError> {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| storage_err("HTTP", e))?;
        Ok(FetchClients {
            http,
            rss_base: rss_base.to_string(),
            oembed_base: oembed_base.to_string(),
            api_base: api_base.to_string(),
        })
    }
}

/// HTTP result distinguishing 304 Not Modified (RSS ETag path).
pub enum HttpResponse {
    Body(reqwest::Response),
    NotModified,
}

/// Execute `attempt` with 3× exponential-backoff retries on retryable
/// statuses (429, 5xx) and transport timeouts. Non-retryable statuses are
/// returned as a `Fetch` error carrying the status (bodies of failed
/// responses are dropped — the 403 quotaExceeded path re-issues its own
/// request inside the API client, see `api.rs`).
pub(crate) async fn retry_http<F, Fut>(
    src: Source,
    url: &str,
    mut attempt: F,
) -> Result<HttpResponse, TubeforgeError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<reqwest::Response, reqwest::Error>>,
{
    let mut delay = Duration::from_millis(400);
    let mut attempts: u32 = 0;
    loop {
        attempts += 1;
        let resp = match attempt().await {
            Ok(r) => r,
            Err(e) => {
                if e.is_timeout() && attempts <= MAX_RETRIES {
                    tokio::time::sleep(delay).await;
                    delay = delay.saturating_mul(2);
                    continue;
                }
                return Err(TubeforgeError::Fetch {
                    src,
                    url: url.to_string(),
                    inner: e.to_string(),
                });
            }
        };
        let status = resp.status();
        if status.is_success() {
            return Ok(HttpResponse::Body(resp));
        }
        if status == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(HttpResponse::NotModified);
        }
        if is_retryable(status) && attempts <= MAX_RETRIES {
            tokio::time::sleep(delay).await;
            delay = delay.saturating_mul(2);
            continue;
        }
        return Err(TubeforgeError::Fetch {
            src,
            url: url.to_string(),
            inner: format!("HTTP {status}"),
        });
    }
}

fn is_retryable(status: reqwest::StatusCode) -> bool {
    matches!(
        status,
        reqwest::StatusCode::TOO_MANY_REQUESTS
            | reqwest::StatusCode::INTERNAL_SERVER_ERROR
            | reqwest::StatusCode::BAD_GATEWAY
            | reqwest::StatusCode::SERVICE_UNAVAILABLE
            | reqwest::StatusCode::GATEWAY_TIMEOUT
    )
}
