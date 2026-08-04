//! YouTube Data API v3 client (LLD §5.3).
//!
//! **Only** `videos.list` (1 unit/call, ≤50 ids per call) plus the handle
//! resolution helper `channels.list(forHandle=...)`. `search.list` is NEVER
//! used. Calls are recorded in the quota ledger (LLD §5.4).

use std::time::Duration;

use serde::Deserialize;
use url::Url;

use super::is_retryable;
use super::quota;
use super::{FetchClients, MAX_RETRIES};
use crate::error::{Endpoint, Source, TubeforgeError};
use crate::storage::Db;

/// Max ids per `videos.list` call (LLD §5.3: ≤50).
pub const BATCH_MAX: usize = 50;

/// Quota model note: Google bills `videos.list` **per part per call**
/// (snippet/contentDetails/statistics are 2 units each, recordingDetails and
/// topicDetails 1 unit each, per the cost table). TubeForge deliberately
/// keeps the simpler ledger model of **1 unit per call** (LLD §5.3, README)
/// — conservative (it overcounts vs. per-part billing) and pinned by the
/// quota tests. If per-part billing is ever adopted, the cost must be
/// computed at the `quota::record_videos_list_calls` call site instead.
const PART: &str = "snippet,contentDetails,statistics,recordingDetails,topicDetails";
const FIELDS: &str = "items(id,snippet(title,description,tags,categoryId,publishedAt,\
                      channelId,channelTitle,thumbnails(default(url))),\
                      contentDetails(duration),statistics(viewCount,likeCount,commentCount),\
                      recordingDetails(location(latitude,longitude,locationDescription),recordingDate),\
                      topicDetails(topicCategories))";
const CHANNELS_FIELDS: &str = "items(id,snippet(title))";

/// `videos.list` parts/fields for `check availability`: `status` only is
/// enough for privacyStatus, but the spec calls for `part=snippet,status`
/// and the snippet carries the channel id used to attach the alert.
const AVAILABILITY_PART: &str = "snippet,status";
const AVAILABILITY_FIELDS: &str = "items(id,snippet(channelId),status(privacyStatus))";

/// Rich metadata for one video (snippet + contentDetails + statistics +
/// recordingDetails + topicDetails — the two free GEO signals, C1/C2).
#[derive(Debug, Clone, Default)]
pub struct ApiVideo {
    pub video_id: String,
    pub channel_id: Option<String>,
    pub channel_title: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub category_id: Option<String>,
    pub duration_sec: Option<i64>,
    pub published_at: Option<String>,
    pub view_count: Option<i64>,
    pub like_count: Option<i64>,
    pub comment_count: Option<i64>,
    pub thumb_url: Option<String>,
    /// `recordingDetails.recordingDate` — date-only RFC3339 (YouTube Studio
    /// lets creators pick a date, no time component; MW Metadata §recordingDetails).
    pub recording_date: Option<String>,
    /// `recordingDetails.location.locationDescription` (e.g. "Googleplex").
    pub recording_location_name: Option<String>,
    pub recording_lat: Option<f64>,
    pub recording_lng: Option<f64>,
    /// `topicDetails.topicCategories` — Wikipedia category URLs (MW Metadata
    /// §topicDetails); labels are derived at read time.
    pub topic_categories: Vec<String>,
}

/// One `videos.list` availability snapshot: id + optional channel + the
/// `status.privacyStatus` value (`public` | `unlisted` | `private`).
#[derive(Debug, Clone, Default)]
pub struct AvailabilityItem {
    pub video_id: String,
    pub channel_id: Option<String>,
    pub privacy_status: Option<String>,
}

pub struct ApiClient {
    clients: FetchClients,
    key: String,
}

impl ApiClient {
    pub fn new(clients: &FetchClients, key: &str) -> Self {
        ApiClient {
            clients: FetchClients {
                http: clients.http.clone(),
                rss_base: clients.rss_base.clone(),
                oembed_base: clients.oembed_base.clone(),
                api_base: clients.api_base.clone(),
            },
            key: key.to_string(),
        }
    }

    /// Batch `videos.list` for up to 50 ids per call, recording 1 unit per
    /// call in the quota ledger. `403 quotaExceeded` → `Quota` error.
    pub async fn fetch_videos(
        &self,
        db: &Db,
        ids: &[String],
    ) -> Result<Vec<ApiVideo>, TubeforgeError> {
        let mut out = Vec::new();
        for chunk in ids.chunks(BATCH_MAX) {
            let resp = self.request_videos(chunk).await?;
            out.extend(resp.items);
            // 1 unit per call, regardless of ids in it (LLD §5.3).
            quota::record_videos_list_calls(db, 1).await?;
        }
        Ok(out)
    }

    /// `check availability` path: batched `videos.list` with
    /// `part=snippet,status` for the privacy snapshot. Only videos that
    /// still exist come back; deleted/private-hidden ids are simply absent
    /// from the response (YouTube also reports them as a `videoNotFound`
    /// API error — see LLD §5.3 API behavior notes).
    ///
    /// Missing-ids are the CALLER's concern (alerts, not errors — the
    /// command treats "absent" as a finding, never a failure). To honor
    /// that, an HTTP 400/404 whose body carries a `videoNotFound` reason is
    /// mapped to an empty result (every requested id counts as missing)
    /// instead of a hard `Fetch` error; any other non-success status
    /// propagates like the normal API path. 1 unit/call is recorded in the
    /// ledger either way.
    ///
    /// (Future unlisted-discovery path, documented per PRD research: the
    /// playlistItems `videoOwnerChannelId` heuristic — an unlisted video
    /// still surfaces through a channel's uploads playlist item, so
    /// `playlistItems.list` can catch what `videos.list` hides. Not
    /// implemented here: it costs extra quota per channel and unlisted
    /// videos are a deliberate uploader choice, not an availability
    /// failure.)
    pub async fn fetch_availability(
        &self,
        db: &Db,
        ids: &[String],
    ) -> Result<Vec<AvailabilityItem>, TubeforgeError> {
        let mut out = Vec::new();
        for chunk in ids.chunks(BATCH_MAX) {
            out.extend(self.request_availability(chunk).await?);
            // 1 unit per call (LLD §5.3), recorded even when the batch
            // comes back empty (deleted/private videos).
            quota::record_videos_list_calls(db, 1).await?;
        }
        Ok(out)
    }

    /// One `videos.list` availability call. The 404/400-with-`videoNotFound`
    /// body is folded into an empty result (see `fetch_availability`).
    /// Mirrors `request()` (3× backoff, 403 quotaExceeded → `Quota`) but
    /// keeps 400/404 responses readable instead of erroring on them.
    async fn request_availability(
        &self,
        ids: &[String],
    ) -> Result<Vec<AvailabilityItem>, TubeforgeError> {
        let url = Url::parse_with_params(
            &format!("{}/videos", self.clients.api_base),
            &[
                ("part", AVAILABILITY_PART),
                ("id", &ids.join(",")),
                ("fields", AVAILABILITY_FIELDS),
                ("key", self.key.as_str()),
            ],
        )
        .map_err(|e| TubeforgeError::Fetch {
            src: Source::Api,
            url: "videos.list".to_string(),
            inner: format!("build url: {e}"),
        })?;
        let url_s = url.to_string();

        let mut delay = Duration::from_millis(400);
        let mut attempts: u32 = 0;
        loop {
            attempts += 1;
            let resp = self.clients.http.get(&url_s).send().await.map_err(|e| {
                TubeforgeError::Fetch {
                    src: Source::Api,
                    url: url_s.clone(),
                    inner: e.to_string(),
                }
            })?;
            let status = resp.status();
            if status.is_success() {
                return parse_availability_body(&url_s, resp.text().await).await;
            }
            if status == reqwest::StatusCode::BAD_REQUEST
                || status == reqwest::StatusCode::NOT_FOUND
            {
                // Fold the documented "video(s) gone" error into an empty
                // result so the command can raise alerts instead of failing.
                let body = resp.text().await.unwrap_or_default();
                if body.contains("videoNotFound") {
                    return Ok(Vec::new());
                }
                return Err(TubeforgeError::Fetch {
                    src: Source::Api,
                    url: url_s,
                    inner: format!("HTTP {status} {body}"),
                });
            }
            if status == reqwest::StatusCode::FORBIDDEN {
                let body = resp.text().await.unwrap_or_default();
                if body.contains("quotaExceeded") {
                    return Err(TubeforgeError::Quota {
                        endpoint: Endpoint::VideosList,
                        remaining: 0,
                    });
                }
                return Err(TubeforgeError::Fetch {
                    src: Source::Api,
                    url: url_s,
                    inner: format!("HTTP 403 {body}"),
                });
            }
            if is_retryable(status) && attempts <= MAX_RETRIES {
                tokio::time::sleep(delay).await;
                delay = delay.saturating_mul(2);
                continue;
            }
            return Err(TubeforgeError::Fetch {
                src: Source::Api,
                url: url_s,
                inner: format!("HTTP {status}"),
            });
        }
    }

    /// Resolve `@handle` → channel id via `channels.list(forHandle=...)`
    /// (LLD §6.1). Records no videos.list quota (separate endpoint).
    pub async fn resolve_handle(&self, handle: &str) -> Result<String, TubeforgeError> {
        let url = Url::parse_with_params(
            &format!("{}/channels", self.clients.api_base),
            &[
                ("part", "id,snippet"),
                ("forHandle", handle),
                ("fields", CHANNELS_FIELDS),
                ("key", self.key.as_str()),
            ],
        )
        .map_err(|e| TubeforgeError::Fetch {
            src: Source::Api,
            url: handle.to_string(),
            inner: format!("build url: {e}"),
        })?;
        let url_s = url.to_string();

        let resp = self.request(&url_s, Source::Api).await?;
        let body = resp
            .text()
            .await
            .map_err(|e| TubeforgeError::Fetch {
                src: Source::Api,
                url: url_s.clone(),
                inner: format!("read body: {e}"),
            })?;
        let parsed: ApiChannelsResponse = serde_json::from_str(&body).map_err(|e| {
            TubeforgeError::Parse {
                src: Source::Api,
                item: url_s.clone(),
                inner: format!("json: {e}"),
            }
        })?;
        parsed
            .items
            .into_iter()
            .next()
            .map(|i| i.id)
            .ok_or_else(|| TubeforgeError::Fetch {
                src: Source::Api,
                url: url_s,
                inner: format!("channel not found for handle {handle}"),
            })
    }

    /// One `videos.list` call with `fields` projection (bounded payload).
    async fn request_videos(&self, ids: &[String]) -> Result<ApiVideosResponse, TubeforgeError> {
        let url = Url::parse_with_params(
            &format!("{}/videos", self.clients.api_base),
            &[
                ("part", PART),
                ("id", &ids.join(",")),
                ("fields", FIELDS),
                ("key", self.key.as_str()),
            ],
        )
        .map_err(|e| TubeforgeError::Fetch {
            src: Source::Api,
            url: "videos.list".to_string(),
            inner: format!("build url: {e}"),
        })?;
        let url_s = url.to_string();

        let resp = self.request(&url_s, Source::Api).await?;
        let body = resp
            .text()
            .await
            .map_err(|e| TubeforgeError::Fetch {
                src: Source::Api,
                url: url_s.clone(),
                inner: format!("read body: {e}"),
            })?;
        serde_json::from_str(&body).map_err(|e| TubeforgeError::Parse {
            src: Source::Api,
            item: url_s,
            inner: format!("json: {e}"),
        })
    }

    /// Issue one GET with 3× backoff on 429/5xx/timeout. A 403 with a
    /// `quotaExceeded` body maps to `Quota` (exit 4); other 4xx are `Fetch`.
    async fn request(
        &self,
        url: &str,
        src: Source,
    ) -> Result<reqwest::Response, TubeforgeError> {
        let mut delay = Duration::from_millis(400);
        let mut attempts: u32 = 0;
        loop {
            attempts += 1;
            let resp = self.clients.http.get(url).send().await.map_err(|e| {
                TubeforgeError::Fetch {
                    src,
                    url: url.to_string(),
                    inner: e.to_string(),
                }
            })?;
            let status = resp.status();
            if status.is_success() {
                return Ok(resp);
            }
            if status == reqwest::StatusCode::FORBIDDEN {
                // Body is needed to tell quotaExceeded apart from other 403s.
                let body = resp.text().await.unwrap_or_default();
                if body.contains("quotaExceeded") {
                    return Err(TubeforgeError::Quota {
                        endpoint: Endpoint::VideosList,
                        remaining: 0,
                    });
                }
                return Err(TubeforgeError::Fetch {
                    src,
                    url: url.to_string(),
                    inner: format!("HTTP 403 {body}"),
                });
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
}

/// Parse ISO-8601 duration (`PT1H2M3S`, `P1DT2H`) to seconds.
pub fn iso8601_duration_to_secs(d: &str) -> Option<i64> {
    let mut total: f64 = 0.0;
    let mut num = String::new();
    let mut in_time = false;
    for ch in d.chars().skip(1) {
        match ch {
            '0'..='9' | '.' => num.push(ch),
            'T' => in_time = true,
            'H' => {
                total += num.parse::<f64>().ok()? * 3600.0;
                num.clear();
            }
            'M' => {
                if !in_time {
                    return None; // months unsupported (never sent by YouTube)
                }
                total += num.parse::<f64>().ok()? * 60.0;
                num.clear();
            }
            'S' => {
                total += num.parse::<f64>().ok()?;
                num.clear();
            }
            'D' => {
                total += num.parse::<f64>().ok()? * 86_400.0;
                num.clear();
            }
            _ => return None,
        }
    }
    Some(total.round() as i64)
}

// ---------------------------------------------------------------------------
// Response shapes (fields projection keeps these small)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ApiVideosResponse {
    #[serde(default)]
    items: Vec<ApiVideo>,
}

/// `check availability` response: only existing videos appear in `items`.
#[derive(Debug, Deserialize)]
struct AvailabilityResponse {
    #[serde(default)]
    items: Vec<AvailabilityRawItem>,
}

#[derive(Debug, Deserialize)]
struct AvailabilityRawItem {
    id: String,
    #[serde(default)]
    snippet: Option<AvailabilitySnippet>,
    #[serde(default)]
    status: Option<AvailabilityStatus>,
}

#[derive(Debug, Deserialize)]
struct AvailabilitySnippet {
    #[serde(default, rename = "channelId")]
    channel_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AvailabilityStatus {
    #[serde(default, rename = "privacyStatus")]
    privacy_status: Option<String>,
}

/// Parse a 200 availability body. A body that is not JSON at all is a
/// `Parse` error (documented taxonomy — never a panic).
async fn parse_availability_body(
    url: &str,
    body: Result<String, reqwest::Error>,
) -> Result<Vec<AvailabilityItem>, TubeforgeError> {
    let body = body.map_err(|e| TubeforgeError::Fetch {
        src: Source::Api,
        url: url.to_string(),
        inner: format!("read body: {e}"),
    })?;
    let parsed: AvailabilityResponse = serde_json::from_str(&body).map_err(|e| {
        TubeforgeError::Parse {
            src: Source::Api,
            item: url.to_string(),
            inner: format!("json: {e}"),
        }
    })?;
    Ok(parsed
        .items
        .into_iter()
        .map(|i| AvailabilityItem {
            video_id: i.id,
            channel_id: i.snippet.and_then(|s| s.channel_id),
            privacy_status: i.status.and_then(|s| s.privacy_status),
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct ApiChannelsResponse {
    #[serde(default)]
    items: Vec<ApiChannelItem>,
}

#[derive(Debug, Deserialize)]
struct ApiChannelItem {
    id: String,
}

impl ApiVideo {
    /// The `statistics.*` values are strings in the YouTube API; parse them
    /// into integers here so the storage layer stores plain integers.
    fn from_item(item: RawVideoItem) -> Self {
        ApiVideo {
            video_id: item.id,
            channel_id: item.snippet.as_ref().and_then(|s| s.channel_id.clone()),
            channel_title: item.snippet.as_ref().and_then(|s| s.channel_title.clone()),
            title: item.snippet.as_ref().and_then(|s| s.title.clone()),
            description: item.snippet.as_ref().and_then(|s| s.description.clone()),
            tags: item.snippet.as_ref().map(|s| s.tags.clone()).unwrap_or_default(),
            category_id: item.snippet.as_ref().and_then(|s| s.category_id.clone()),
            published_at: item
                .snippet
                .as_ref()
                .and_then(|s| s.published_at.clone()),
            thumb_url: item
                .snippet
                .as_ref()
                .and_then(|s| s.thumbnails.as_ref())
                .and_then(|t| t.default_thumb.as_ref())
                .and_then(|t| t.url.clone()),
            duration_sec: item
                .content_details
                .as_ref()
                .and_then(|c| c.duration.as_deref())
                .and_then(iso8601_duration_to_secs),
            view_count: item
                .statistics
                .as_ref()
                .and_then(|s| s.view_count.as_deref())
                .and_then(|v| v.parse().ok()),
            like_count: item
                .statistics
                .as_ref()
                .and_then(|s| s.like_count.as_deref())
                .and_then(|v| v.parse().ok()),
            comment_count: item
                .statistics
                .as_ref()
                .and_then(|s| s.comment_count.as_deref())
                .and_then(|v| v.parse().ok()),
            recording_date: item
                .recording_details
                .as_ref()
                .and_then(|r| r.recording_date.clone()),
            recording_location_name: item
                .recording_details
                .as_ref()
                .and_then(|r| r.location.as_ref())
                .and_then(|l| l.location_description.clone()),
            recording_lat: item
                .recording_details
                .as_ref()
                .and_then(|r| r.location.as_ref())
                .and_then(|l| l.latitude),
            recording_lng: item
                .recording_details
                .as_ref()
                .and_then(|r| r.location.as_ref())
                .and_then(|l| l.longitude),
            topic_categories: item
                .topic_details
                .as_ref()
                .map(|t| t.topic_categories.clone())
                .unwrap_or_default(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawVideoItem {
    id: String,
    #[serde(default)]
    snippet: Option<RawSnippet>,
    #[serde(default, rename = "contentDetails")]
    content_details: Option<RawContentDetails>,
    #[serde(default)]
    statistics: Option<RawStatistics>,
    #[serde(default, rename = "recordingDetails")]
    recording_details: Option<RawRecordingDetails>,
    #[serde(default, rename = "topicDetails")]
    topic_details: Option<RawTopicDetails>,
}

#[derive(Debug, Deserialize)]
struct RawRecordingDetails {
    #[serde(default)]
    location: Option<RawRecordingLocation>,
    #[serde(default, rename = "recordingDate")]
    recording_date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawRecordingLocation {
    #[serde(default)]
    latitude: Option<f64>,
    #[serde(default)]
    longitude: Option<f64>,
    #[serde(default, rename = "locationDescription")]
    location_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawTopicDetails {
    #[serde(default, rename = "topicCategories")]
    topic_categories: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawSnippet {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default, rename = "categoryId")]
    category_id: Option<String>,
    #[serde(default, rename = "publishedAt")]
    published_at: Option<String>,
    #[serde(default, rename = "channelId")]
    channel_id: Option<String>,
    #[serde(default, rename = "channelTitle")]
    channel_title: Option<String>,
    #[serde(default)]
    thumbnails: Option<RawThumbnails>,
}

#[derive(Debug, Deserialize)]
struct RawThumbnails {
    #[serde(default, rename = "default")]
    default_thumb: Option<RawThumb>,
}

#[derive(Debug, Deserialize)]
struct RawThumb {
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawContentDetails {
    #[serde(default)]
    duration: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawStatistics {
    #[serde(default, rename = "viewCount")]
    view_count: Option<String>,
    #[serde(default, rename = "likeCount")]
    like_count: Option<String>,
    #[serde(default, rename = "commentCount")]
    comment_count: Option<String>,
}

impl<'de> Deserialize<'de> for ApiVideo {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        RawVideoItem::deserialize(deserializer).map(ApiVideo::from_item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-built `videos.list` response (C1/C2 shape): recordingDetails with
    /// location + recordingDate, and topicDetails with category URLs. The
    /// second item carries neither part — the mapping must degrade to
    /// None/empty instead of failing.
    #[test]
    fn recording_details_and_topics_parse_from_response() {
        let body = r#"{
          "items": [{
            "id": "aaa111bbb22",
            "snippet": {"publishedAt": "2026-07-15T10:00:00Z", "title": "t",
                        "channelId": "UCx", "channelTitle": "c"},
            "recordingDetails": {
              "location": {"latitude": 37.422, "longitude": -122.084,
                           "locationDescription": "Googleplex"},
              "recordingDate": "2026-07-10T00:00:00Z"
            },
            "topicDetails": {
              "topicCategories": [
                "https://en.wikipedia.org/wiki/Artificial_intelligence",
                "https://en.wikipedia.org/wiki/Deep_learning"
              ]
            }
          }, {
            "id": "bbb222ccc33",
            "snippet": {"publishedAt": "2026-07-01T08:00:00Z", "title": "t2"}
          }]
        }"#;
        let resp: ApiVideosResponse = serde_json::from_str(body).expect("parse");
        assert_eq!(resp.items.len(), 2);
        let a = &resp.items[0];
        assert_eq!(a.recording_date.as_deref(), Some("2026-07-10T00:00:00Z"));
        assert_eq!(a.recording_location_name.as_deref(), Some("Googleplex"));
        assert_eq!(a.recording_lat, Some(37.422));
        assert_eq!(a.recording_lng, Some(-122.084));
        assert_eq!(
            a.topic_categories,
            vec![
                "https://en.wikipedia.org/wiki/Artificial_intelligence",
                "https://en.wikipedia.org/wiki/Deep_learning"
            ]
        );
        // Absent parts → None / empty (never a parse failure).
        assert_eq!(resp.items[1].recording_date, None);
        assert_eq!(resp.items[1].recording_location_name, None);
        assert_eq!(resp.items[1].recording_lat, None);
        assert!(resp.items[1].topic_categories.is_empty());
    }
}
