//! oEmbed fetch for single videos (LLD §5.2).
//!
//! URL: `https://www.youtube.com/oembed?url=...&format=json`.
//! Fields: title, author_name, author_url, thumbnail_url — NO description,
//! views or date; oEmbed-sourced videos are thin by design (no key mode).

use serde::Deserialize;
use url::Url;

use super::retry_http;
use super::{FetchClients, HttpResponse};
use crate::error::{Source, TubeforgeError};

/// oEmbed payload (YouTube `video` type subset, LLD §5.2).
#[derive(Debug, Clone, Deserialize)]
pub struct OEmbed {
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub title: Option<String>,
    #[serde(rename = "author_name")]
    pub author_name: Option<String>,
    #[serde(rename = "author_url")]
    pub author_url: Option<String>,
    #[serde(rename = "thumbnail_url")]
    pub thumbnail_url: Option<String>,
}

impl OEmbed {
    /// `@handle` extracted from `author_url` (`https://youtube.com/@name`).
    /// Returns None for legacy `/channel/UC...` author URLs.
    pub fn handle(&self) -> Option<String> {
        self.author_url.as_deref().and_then(handle_from_author_url)
    }
}

/// Extract the `@handle` from a channel author URL. Only the `@`-prefixed
/// path form is a handle; `/channel/UC...` and `/c/...` URLs yield None.
pub fn handle_from_author_url(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let segment = parsed.path_segments()?.next_back()?.to_string();
    if let Some(rest) = segment.strip_prefix('@') {
        if !rest.is_empty() {
            return Some(format!("@{rest}"));
        }
    }
    None
}

/// Fetch oEmbed metadata for a video id. Never touches quota (zero cost).
pub async fn fetch(clients: &FetchClients, video_id: &str) -> Result<OEmbed, TubeforgeError> {
    let watch = format!("https://www.youtube.com/watch?v={video_id}");
    let url = Url::parse_with_params(
        &format!("{}/oembed", clients.oembed_base),
        [("url", watch.as_str()), ("format", "json")],
    )
    .map_err(|e| TubeforgeError::Fetch {
        src: Source::OEmbed,
        url: video_id.to_string(),
        inner: format!("build url: {e}"),
    })?;
    let url_s = url.to_string();

    let resp = retry_http(Source::OEmbed, &url_s, || clients.http.get(&url_s).send()).await?;
    let HttpResponse::Body(resp) = resp else {
        return Err(TubeforgeError::Fetch {
            src: Source::OEmbed,
            url: url_s,
            inner: "unexpected 304 Not Modified".to_string(),
        });
    };

    let body = resp.text().await.map_err(|e| TubeforgeError::Fetch {
        src: Source::OEmbed,
        url: url_s.clone(),
        inner: format!("read body: {e}"),
    })?;

    serde_json::from_str(&body).map_err(|e| TubeforgeError::Parse {
        src: Source::OEmbed,
        item: url_s,
        inner: format!("json: {e}"),
    })
}
