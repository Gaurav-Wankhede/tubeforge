//! `metadata` (Phase 6.6): yt-dlp metadata enrichment — the performance
//! half's data source. Extracts the full keyless payload (heatmap /
//! audience-retention curve, live view/like/comment counts, channel
//! followers) and persists:
//! - `video_heatmap` (migration 007) — the 100-point retention curve,
//!   the ONLY public retention data source (the API cannot provide it)
//! - `videos.view_count/like_count/comment_count` refreshed to live values
//! - `channels.subscriber_count` (channel_follower_count) backfill
//!
//! Requires `TUBEFORGE_YTDLP_ENABLED=true` (Config error otherwise).

use std::sync::Arc;
use futures::StreamExt;
use serde_json::{json, Value};
use tokio::sync::Semaphore;

use crate::config::Config;
use crate::error::TubeforgeError;
use crate::fetch::ytdlp::YtdlpClient;
use crate::fetch::FetchClients;
use crate::storage::db::Db;
use crate::util;

/// `metadata`: enrich stored videos keylessly.
pub async fn run(
    cfg: &Config,
    video_id: Option<String>,
    all: bool,
    limit: Option<usize>,
) -> Result<Value, TubeforgeError> {
    if let Some(vid) = video_id {
        run_single(cfg, &vid).await
    } else if all || limit.is_some() {
        run_batch(cfg, limit).await
    } else {
        Err(TubeforgeError::Usage(
            "specify --video-id <ID> or --all [--limit N]".to_string(),
        ))
    }
}

/// Enrich one stored video keylessly.
pub async fn run_single(cfg: &Config, video_id: &str) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;

    let stored = db.all_videos().await?;
    if !stored.iter().any(|v| v.video_id == video_id) {
        return Err(TubeforgeError::Usage(format!(
            "video not in database: {video_id} — `metadata` only covers stored videos"
        )));
    }

    let client = YtdlpClient::new(
        cfg.ytdlp_path.clone(),
        cfg.ytdlp_enabled,
        cfg.ytdlp_client.clone(),
        cfg.ytdlp_js_runtime.clone(),
    )?;
    let info = client.metadata(video_id).await?;
    let now = util::now_rfc3339();

    // 1. Heatmap (audience retention curve) → video_heatmap.
    crate::analytics::performance::persist_heatmap(&db, video_id, &info.heatmap, &now).await?;

    // 2. Live stats refresh on the video row.
    if info.view_count.is_some() || info.like_count.is_some() || info.comment_count.is_some() {
        db.update_video_stats(
            video_id,
            info.view_count,
            info.like_count,
            info.comment_count,
            &now,
        )
        .await?;
    }

    // 3. Channel follower backfill (channels.subscriber_count).
    if let (Some(cid), Some(followers)) = (&info.channel_id, info.channel_follower_count) {
        db.update_channel_subscribers(cid, followers, &now).await?;
    }

    // 4. Tags: persist into videos.tags and tags/video_tags tables.
    let tags_persisted = if !info.tags.is_empty() {
        let tags_json = serde_json::to_string(&info.tags).unwrap_or_else(|_| "[]".to_string());
        db.set_video_tags(video_id, &tags_json, &now).await?;
        db.upsert_tags(video_id, &info.tags, "youtube").await?;
        info.tags.len()
    } else {
        0
    };

    Ok(json!({
        "video_id": video_id,
        "title": info.title,
        "views": info.view_count,
        "likes": info.like_count,
        "comments": info.comment_count,
        "tags_persisted": tags_persisted,
        "channel_followers": info.channel_follower_count,
        "heatmap_points": info.heatmap.len(),
        "captions_langs": info.automatic_captions.len(),
        "tags": info.tags.len(),
        "note": "heatmap + live stats stored — retention signals now score the video",
    }))
}

/// Enrich all unenriched stored videos concurrently in batch.
pub async fn run_batch(cfg: &Config, limit: Option<usize>) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    let clients = FetchClients::new()?;
    let stored = db.all_videos().await?;
    let now = util::now_rfc3339();

    let targets: Vec<_> = stored
        .into_iter()
        .filter(|v| v.like_count.is_none() || v.tags == "[]" || v.tags.is_empty())
        .take(limit.unwrap_or(usize::MAX))
        .collect();

    let total_targets = targets.len();
    let mut enriched = 0;

    let sem = Arc::new(Semaphore::new(16));
    let mut tasks = futures::stream::FuturesUnordered::new();

    for v in targets {
        let sem = sem.clone();
        let clients_ref = &clients;
        let vid = v.video_id.clone();
        tasks.push(async move {
            let _permit = sem.acquire().await.ok();
            let meta = crate::fetch::innertube::fetch_video_meta(clients_ref, &vid).await;
            (vid, meta)
        });
    }

    while let Some((vid, meta)) = tasks.next().await {
        if let Ok(info) = meta {
            if info.view_count.is_some() || info.like_count.is_some() || info.comment_count.is_some() {
                let _ = db
                    .update_video_stats(
                        &vid,
                        info.view_count,
                        info.like_count,
                        info.comment_count,
                        &now,
                    )
                    .await;
            }
            if !info.tags.is_empty() {
                let tags_json =
                    serde_json::to_string(&info.tags).unwrap_or_else(|_| "[]".to_string());
                let _ = db.set_video_tags(&vid, &tags_json, &now).await;
                let _ = db.upsert_tags(&vid, &info.tags, "youtube").await;
            }
            enriched += 1;
        }
    }

    Ok(json!({
        "total_targets": total_targets,
        "enriched": enriched,
        "status": "ok"
    }))
}
