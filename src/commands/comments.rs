//! `comments` (Phase 6.5): fetch top-level comments via yt-dlp
//! (`--write-comments`, keyless — InnerTube continuation API, zero quota)
//! with the YouTube Data API `commentThreads.list` as an opt-in fallback
//! (`--api`, 1 unit/call + 1 unit/page in the shared quota ledger).
//! Comment mining is Method B of the 2026 gap-mining research — the
//! highest-confidence demand signal, and the input for the Comment+Transcript
//! prompt bundle.

use serde_json::{json, Value};

use crate::config::Config;
use crate::error::TubeforgeError;
use crate::fetch::api::{ApiClient, ApiComment};
use crate::fetch::ytdlp::YtdlpClient;
use crate::fetch::FetchClients;
use crate::storage::db::{CommentRow, Db};
use crate::util;

/// `comments get --video-id X [--max N] [--api]`: fetch + store top-level
/// comments. Default source is yt-dlp (keyless); `--api` forces the YouTube
/// Data API (needs YOUTUBE_API_KEY).
pub async fn run_get(
    cfg: &Config,
    video_id: &str,
    max: u64,
    use_api: bool,
) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;

    let stored = db.all_videos().await?;
    if !stored.iter().any(|v| v.video_id == video_id) {
        return Err(TubeforgeError::Usage(format!(
            "video not in database: {video_id} — `comments get` only covers stored videos"
        )));
    }

    let now = util::now_rfc3339();
    let rows: Vec<CommentRow> = if use_api {
        let key = cfg.youtube_api_key.as_deref().ok_or_else(|| {
            TubeforgeError::Config(
                "comments get --api needs YOUTUBE_API_KEY in .env (YouTube Data API v3 key)"
                    .to_string(),
            )
        })?;
        let clients = FetchClients::new()?;
        let api = ApiClient::new(&clients, key);
        let fetched: Vec<ApiComment> = api.fetch_comments(&db, video_id, max).await?;
        fetched
            .iter()
            .map(|c| CommentRow {
                comment_id: c.comment_id.clone(),
                video_id: video_id.to_string(),
                author: c.author.clone(),
                text: c.text.clone(),
                like_count: c.like_count,
                published_at: c.published_at.clone().unwrap_or_default(),
                fetched_at: now.clone(),
            })
            .collect()
    } else {
        let client = YtdlpClient::new(
            cfg.ytdlp_path.clone(),
            cfg.ytdlp_enabled,
            cfg.ytdlp_client.clone(),
            cfg.ytdlp_js_runtime.clone(),
        )?;
        let fetched = client.comments(video_id, max).await?;
        fetched
            .iter()
            .map(|c| CommentRow {
                comment_id: c.comment_id.clone(),
                video_id: video_id.to_string(),
                author: c.author.clone(),
                text: c.text.clone(),
                like_count: c.like_count,
                published_at: c.published_at.clone().unwrap_or_default(),
                fetched_at: now.clone(),
            })
            .collect()
    };

    let inserted = db.upsert_comments(video_id, &rows, &now).await?;

    let top: Vec<Value> = rows
        .iter()
        .take(5)
        .map(|c| {
            json!({
                "author": c.author,
                "text": c.text.chars().take(120).collect::<String>(),
                "likes": c.like_count,
            })
        })
        .collect();

    Ok(json!({
        "video_id": video_id,
        "source": if use_api { "youtube-api" } else { "yt-dlp" },
        "fetched": rows.len(),
        "stored": inserted,
        "top": top,
        "note": "comments stored — include them in a prompt bundle with `tubeforge prompt --video-id <ID> --comments`",
    }))
}

/// `comments list --video-id X`: stored comments, by like count.
pub async fn run_list(cfg: &Config, video_id: &str) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    let rows = db.list_comments(video_id).await?;
    let items: Vec<Value> = rows
        .iter()
        .map(|c| {
            json!({
                "comment_id": c.comment_id,
                "author": c.author,
                "text": c.text.chars().take(200).collect::<String>(),
                "likes": c.like_count,
                "published_at": c.published_at,
            })
        })
        .collect();
    Ok(json!({ "video_id": video_id, "comments": items, "total": items.len() }))
}

/// `comments clear`: wipe the comments table.
pub async fn run_clear(cfg: &Config) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    db.clear_comments().await?;
    Ok(json!({ "cleared": true }))
}
