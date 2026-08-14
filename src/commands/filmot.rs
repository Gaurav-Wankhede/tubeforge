//! `filmot get` (Phase 3 workstream B): look up a video's archived metadata
//! in the Filmot index — an OPT-IN third-party lookup, never part of any
//! ingest path.
//!
//! - Endpoint: `https://filmot.com/api/getvideos?id=<ID>&flags=1&key=<KEY>`
//!   (flags=1 → channel + description included, per MW Metadata usage —
//!   mattwright324/youtube-metadata, MIT). Response is a JSON array; the
//!   first element is the video.
//! - Key: env `TUBEFORGE_FILMOT_KEY`. Empty by default — TubeForge does NOT
//!   embed a third-party key (Filmot's public key sits in MW Metadata's
//!   open-source JS; users who want Filmot bring their own). Missing key is
//!   a clear `Config` error.
//! - Filmot is a third-party service with its own ToS ("limited resources,
//!   do not misuse"); its metadata index covers 18B+ videos but is
//!   stale-by-nature. This command is non-fatal and read-only: NO database
//!   writes, unexpected response shapes degrade to `summary: null` instead
//!   of crashing (no `deny_unknown_fields`, all fields optional). Network
//!   failures exit per the fetch taxonomy (exit 3).

use serde_json::{json, Value};
use url::Url;

use crate::error::{Source, TubeforgeError};
use crate::fetch::FetchClients;

/// The Filmot metadata API base (fixed — no test override needed; the
/// command is opt-in and read-only).
const FILMOT_API: &str = "https://filmot.com/api/getvideos";

/// Env key holding the user's own Filmot API key.
pub const FILMOT_KEY_ENV: &str = "TUBEFORGE_FILMOT_KEY";

pub async fn run_get(video_id: &str) -> Result<Value, TubeforgeError> {
    let key = std::env::var(FILMOT_KEY_ENV)
        .ok()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .ok_or_else(|| {
            TubeforgeError::Config(
                "TUBEFORGE_FILMOT_KEY is not set — `filmot get` needs your own Filmot API key \
                 (filmot.com). TubeForge does not embed Filmot's public key; set the env var \
                 in .env to opt in to this third-party lookup"
                    .to_string(),
            )
        })?;

    let url = Url::parse_with_params(
        FILMOT_API,
        &[("id", video_id), ("flags", "1"), ("key", &key)],
    )
    .map_err(|e| TubeforgeError::Fetch {
        src: Source::Api,
        url: video_id.to_string(),
        inner: format!("build url: {e}"),
    })?;
    let url_s = url.to_string();

    let clients = FetchClients::new()?; // 15s timeout (LLD §5.1).
    let resp = clients
        .http
        .get(&url_s)
        .send()
        .await
        .map_err(|e| TubeforgeError::Fetch {
            src: Source::Api,
            url: url_s.clone(),
            inner: e.to_string(),
        })?;
    if !resp.status().is_success() {
        return Err(TubeforgeError::Fetch {
            src: Source::Api,
            url: url_s,
            inner: format!("HTTP {}", resp.status()),
        });
    }
    let body = resp.text().await.map_err(|e| TubeforgeError::Fetch {
        src: Source::Api,
        url: url_s.clone(),
        inner: format!("read body: {e}"),
    })?;
    // Tolerant parse: the API shape has drifted before and will again. The
    // raw payload always goes through as-is; the summary degrades to null.
    let raw: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            return Err(TubeforgeError::Parse {
                src: Source::Api,
                item: url_s,
                inner: format!("json: {e}"),
            })
        }
    };

    Ok(json!({
        "video_id": video_id,
        "raw": raw,
        "summary": summarize(&raw),
        "note": "Filmot is a third-party index (18B+ videos) — data is stale-by-nature; \
                 no database writes (see `tubeforge filmot --help`)",
    }))
}

/// Extract the first video element (the response is a JSON array; the
/// summary tolerates any other shape → `null`).
fn summarize(raw: &Value) -> Value {
    let video = match raw {
        Value::Array(items) => items.first(),
        _ => None,
    };
    let Some(v) = video else {
        return Value::Null;
    };
    if !v.is_object() {
        return Value::Null;
    }
    let title = v.get("title").and_then(Value::as_str);
    let channel = v.get("channelname").and_then(Value::as_str);
    let channel_id = v.get("channelid").and_then(Value::as_str);
    let upload_date = v.get("uploaddate").and_then(Value::as_str);
    let description_length = v
        .get("description")
        .and_then(Value::as_str)
        .map(|d| d.chars().count());
    json!({
        "title": title,
        "channel": channel,
        "channel_id": channel_id,
        "upload_date": upload_date,
        "description_length": description_length,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_first_array_element() {
        let raw = json!([
            {
                "title": "Rick Astley",
                "channelname": "Rick Astley",
                "channelid": "UCuAXFkgsw1L7xaCfnd5JJOw",
                "uploaddate": "2010-05-01T12:13:14",
                "description": "Never gonna give you up"
            }
        ]);
        let s = summarize(&raw);
        assert_eq!(s["title"], "Rick Astley");
        assert_eq!(s["channel"], "Rick Astley");
        assert_eq!(s["channel_id"], "UCuAXFkgsw1L7xaCfnd5JJOw");
        assert_eq!(s["upload_date"], "2010-05-01T12:13:14");
        assert_eq!(s["description_length"], 23);
    }

    #[test]
    fn summarize_tolerates_unexpected_shapes() {
        // Object instead of array → null summary, no panic.
        assert_eq!(summarize(&json!({"videos": [1]})), Value::Null);
        // Empty array → null summary.
        assert_eq!(summarize(&json!([])), Value::Null);
        // Missing fields → nulls, no panic.
        let s = summarize(&json!([{"title": "only title"}]));
        assert_eq!(s["channel"], Value::Null);
        assert_eq!(s["description_length"], Value::Null);
        // Non-object first element → null summary.
        assert_eq!(summarize(&json!([42])), Value::Null);
    }
}
