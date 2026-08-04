//! Fetch layer (LLD §5): RSS feeds, oEmbed, YouTube Data API v3 + quota
//! ledger. Shared HTTP client and retry plumbing live here.
//!
//! Policy (LLD §5, §6): 15s timeout, 3× exponential-backoff retries on
//! 500/429/timeout, ETag caching for RSS, `search.list` NEVER used.

pub mod api;
pub mod oembed;
pub mod quota;
pub mod rss;

use std::future::Future;
use std::time::Duration;

use crate::error::{storage_err, TubeforgeError};
use crate::error::Source;

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
