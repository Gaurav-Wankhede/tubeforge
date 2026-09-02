//! Native Rust InnerTube Client (Keyless Rich Metadata Extraction with UA Rotation).
//!
//! Queries YouTube's internal JSON API endpoints and watch player responses
//! directly using `reqwest` with HTTP/2 keep-alive, rotating realistic User-Agents
//! and Client Platforms across macOS, Windows, Linux, Android, and iOS to
//! completely prevent rate-limiting and anti-bot bans.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{Source, TubeforgeError};
use crate::fetch::FetchClients;

/// User-Agent & Client Platform definition for rotation.
pub struct UaProfile {
    pub ua: &'static str,
    pub sec_ch_ua: &'static str,
    pub platform: &'static str,
    pub mobile: &'static str,
}

pub const ROTATING_UA_POOL: &[UaProfile] = &[
    UaProfile {
        ua: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
        sec_ch_ua: "\"Chromium\";v=\"124\", \"Google Chrome\";v=\"124\", \"Not-A.Brand\";v=\"99\"",
        platform: "\"macOS\"",
        mobile: "?0",
    },
    UaProfile {
        ua: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
        sec_ch_ua: "\"Chromium\";v=\"124\", \"Google Chrome\";v=\"124\", \"Not-A.Brand\";v=\"99\"",
        platform: "\"Windows\"",
        mobile: "?0",
    },
    UaProfile {
        ua: "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_4_1) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4.1 Safari/605.1.15",
        sec_ch_ua: "\"Not_A Brand\";v=\"8\", \"Safari\";v=\"17\"",
        platform: "\"macOS\"",
        mobile: "?0",
    },
    UaProfile {
        ua: "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:125.0) Gecko/20100101 Firefox/125.0",
        sec_ch_ua: "\"Firefox\";v=\"125\"",
        platform: "\"Windows\"",
        mobile: "?0",
    },
    UaProfile {
        ua: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
        sec_ch_ua: "\"Chromium\";v=\"124\", \"Google Chrome\";v=\"124\", \"Not-A.Brand\";v=\"99\"",
        platform: "\"Linux\"",
        mobile: "?0",
    },
    UaProfile {
        ua: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36 Edg/124.0.0.0",
        sec_ch_ua: "\"Chromium\";v=\"124\", \"Microsoft Edge\";v=\"124\", \"Not-A.Brand\";v=\"99\"",
        platform: "\"Windows\"",
        mobile: "?0",
    },
    UaProfile {
        ua: "Mozilla/5.0 (iPhone; CPU iPhone OS 17_4_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4.1 Mobile/15E148 Safari/604.1",
        sec_ch_ua: "\"Safari\";v=\"17\"",
        platform: "\"iOS\"",
        mobile: "?1",
    },
    UaProfile {
        ua: "Mozilla/5.0 (Linux; Android 14; Pixel 8 Pro) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.6367.113 Mobile Safari/537.36",
        sec_ch_ua: "\"Chromium\";v=\"124\", \"Google Chrome\";v=\"124\", \"Not-A.Brand\";v=\"99\"",
        platform: "\"Android\"",
        mobile: "?1",
    },
];

static UA_INDEX: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Get the next rotating User-Agent profile.
pub fn next_ua_profile() -> &'static UaProfile {
    let idx = UA_INDEX.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % ROTATING_UA_POOL.len();
    &ROTATING_UA_POOL[idx]
}

/// Rich metadata extracted natively from YouTube's InnerTube API.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InnertubeVideoMeta {
    pub video_id: String,
    pub title: String,
    pub description: String,
    pub channel_id: String,
    pub channel_title: String,
    pub duration_seconds: Option<i64>,
    pub view_count: Option<i64>,
    pub like_count: Option<i64>,
    pub comment_count: Option<i64>,
    pub tags: Vec<String>,
    pub category: Option<String>,
    pub published_at: Option<String>,
    pub thumb_url: Option<String>,
}

/// Fetch rich metadata natively via YouTube's watch page player response and InnerTube.
pub async fn fetch_video_meta(
    clients: &FetchClients,
    video_id: &str,
) -> Result<InnertubeVideoMeta, TubeforgeError> {
    let profile = next_ua_profile();

    // Primary High-Speed Path: Direct InnerTube v1/player API with WEB client context
    let url = "https://www.youtube.com/youtubei/v1/player?prettyPrint=false";
    let payload = json!({
        "context": {
            "client": {
                "clientName": "WEB",
                "clientVersion": "2.20240313.01.00",
                "hl": "en",
                "gl": "US"
            }
        },
        "videoId": video_id
    });

    let resp = clients
        .http
        .post(url)
        .header("Content-Type", "application/json")
        .header("User-Agent", profile.ua)
        .header("sec-ch-ua", profile.sec_ch_ua)
        .header("sec-ch-ua-platform", profile.platform)
        .header("sec-ch-ua-mobile", profile.mobile)
        .header("Referer", "https://www.youtube.com/")
        .json(&payload)
        .send()
        .await
        .map_err(|e| TubeforgeError::Fetch {
            src: Source::Api,
            url: url.to_string(),
            inner: format!("InnerTube player request failed: {e}"),
        })?;

    let body: Value = resp.json().await.map_err(|e| TubeforgeError::Parse {
        src: Source::Api,
        item: url.to_string(),
        inner: format!("InnerTube JSON decode failed: {e}"),
    })?;

    let mut meta = InnertubeVideoMeta {
        video_id: video_id.to_string(),
        ..Default::default()
    };

    if let Some(details) = body.get("videoDetails") {
        if let Some(t) = details.get("title").and_then(|v| v.as_str()) {
            meta.title = t.to_string();
        }
        if let Some(d) = details.get("shortDescription").and_then(|v| v.as_str()) {
            meta.description = d.to_string();
        }
        if let Some(cid) = details.get("channelId").and_then(|v| v.as_str()) {
            meta.channel_id = cid.to_string();
        }
        if let Some(author) = details.get("author").and_then(|v| v.as_str()) {
            meta.channel_title = author.to_string();
        }
        if let Some(sec) = details.get("lengthSeconds").and_then(|v| v.as_str()) {
            meta.duration_seconds = sec.parse::<i64>().ok();
        }
        if let Some(views) = details.get("viewCount").and_then(|v| v.as_str()) {
            meta.view_count = views.parse::<i64>().ok();
        }
        if let Some(keywords) = details.get("keywords").and_then(|v| v.as_array()) {
            meta.tags = keywords
                .iter()
                .filter_map(|k| k.as_str().map(|s| s.to_string()))
                .collect();
        }
        if let Some(thumbs) = details.get("thumbnail").and_then(|t| t.get("thumbnails")).and_then(|t| t.as_array()) {
            if let Some(last) = thumbs.last().and_then(|t| t.get("url")).and_then(|u| u.as_str()) {
                meta.thumb_url = Some(last.to_string());
            }
        }
    }

    if let Some(micro) = body.get("microformat").and_then(|m| m.get("playerMicroformatRenderer")) {
        if meta.view_count.is_none() {
            if let Some(v) = micro.get("viewCount").and_then(|v| v.as_str()) {
                meta.view_count = v.parse::<i64>().ok();
            }
        }
        if let Some(pub_date) = micro.get("publishDate").or_else(|| micro.get("uploadDate")).and_then(|v| v.as_str()) {
            meta.published_at = Some(pub_date.to_string());
        }
    }

    // Extract engagement stats (likes, comments) via InnerTube v1/next
    if let Ok(engagement) = fetch_video_engagement(clients, video_id).await {
        if engagement.like_count.is_some() {
            meta.like_count = engagement.like_count;
        }
        if engagement.comment_count.is_some() {
            meta.comment_count = engagement.comment_count;
        }
    }

    Ok(meta)
}

#[derive(Default)]
struct EngagementMeta {
    like_count: Option<i64>,
    comment_count: Option<i64>,
}

async fn fetch_video_engagement(
    clients: &FetchClients,
    video_id: &str,
) -> Result<EngagementMeta, TubeforgeError> {
    let url = "https://www.youtube.com/youtubei/v1/next?prettyPrint=false";
    let payload = json!({
        "context": {
            "client": {
                "clientName": "WEB",
                "clientVersion": "2.20240313.01.00",
                "hl": "en",
                "gl": "US"
            }
        },
        "videoId": video_id
    });

    let resp = clients
        .http
        .post(url)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| TubeforgeError::Fetch {
            src: Source::Api,
            url: url.to_string(),
            inner: format!("InnerTube next request failed: {e}"),
        })?;

    let body: Value = resp.json().await.map_err(|e| TubeforgeError::Parse {
        src: Source::Api,
        item: url.to_string(),
        inner: format!("InnerTube next JSON decode failed: {e}"),
    })?;

    let mut out = EngagementMeta::default();

    // 1. Check direct likeCount / accessibilityText
    if let Some(text) = find_like_accessibility_text(&body) {
        out.like_count = parse_numeric_string(&text);
    } else if let Some(text) = find_json_string(&body, "likeCount") {
        out.like_count = parse_numeric_string(&text);
    }

    // 2. Check commentsCount / commentCount
    if let Some(text) = find_json_string(&body, "commentCount") {
        out.comment_count = parse_numeric_string(&text);
    } else if let Some(text) = find_json_string(&body, "commentsCount") {
        out.comment_count = parse_numeric_string(&text);
    }

    Ok(out)
}

fn find_like_accessibility_text(v: &Value) -> Option<String> {
    match v {
        Value::Object(map) => {
            if let Some(Value::String(s)) = map.get("accessibilityText") {
                if s.contains("like this video along with ") {
                    return Some(s.clone());
                }
            }
            for val in map.values() {
                if let Some(found) = find_like_accessibility_text(val) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => {
            for val in arr {
                if let Some(found) = find_like_accessibility_text(val) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn find_json_string(v: &Value, key: &str) -> Option<String> {
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                if k == key {
                    if let Some(s) = val.as_str() {
                        return Some(s.to_string());
                    } else if let Some(n) = val.as_i64() {
                        return Some(n.to_string());
                    }
                }
                if let Some(found) = find_json_string(val, key) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => {
            for val in arr {
                if let Some(found) = find_json_string(val, key) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn parse_numeric_string(s: &str) -> Option<i64> {
    let clean: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    clean.parse::<i64>().ok()
}
