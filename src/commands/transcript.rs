//! `transcript` (Phase 6.5): fetch + store competitor video transcripts via
//! yt-dlp (public captions — the official captions.download API requires
//! edit permission and cannot read competitor videos; yt-dlp is the
//! industry-standard path per the 2026 gap-mining research).
//!
//! Requires `TUBEFORGE_YTDLP_ENABLED=true` (Config error otherwise) and the
//! yt-dlp binary (`TUBEFORGE_YTDLP_PATH` or on PATH). Transcripts land in
//! the `transcripts` table and feed the `prompt` bundles for AI gap mining.

use serde_json::{json, Value};

use crate::config::Config;
use crate::error::TubeforgeError;
use crate::fetch::ytdlp::YtdlpClient;
use crate::storage::db::Db;
use crate::util;

/// `transcript get --video-id X [--lang en]`: fetch + store one transcript.
pub async fn run_get(cfg: &Config, video_id: &str, lang: &str) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;

    // Validate the video is stored (consistent with `check availability`:
    // unknown ids are usage errors, not silent no-ops).
    let stored = db.all_videos().await?;
    if !stored.iter().any(|v| v.video_id == video_id) {
        return Err(TubeforgeError::Usage(format!(
            "video not in database: {video_id} — `transcript get` only covers stored videos"
        )));
    }

    let client = YtdlpClient::new(
        cfg.ytdlp_path.clone(),
        cfg.ytdlp_enabled,
        cfg.ytdlp_client.clone(),
        cfg.ytdlp_js_runtime.clone(),
    )?;
    let (text, kind) = client.transcript(video_id, lang).await?;

    let source = match kind {
        crate::fetch::ytdlp::TranscriptKind::Auto => "auto",
        crate::fetch::ytdlp::TranscriptKind::Manual => "manual",
    };
    let now = util::now_rfc3339();
    db.upsert_transcript(video_id, lang, source, &text, &now)
        .await?;

    let words = text.split_whitespace().count();
    let preview: String = text.chars().take(200).collect();
    Ok(json!({
        "video_id": video_id,
        "lang": lang,
        "source": source,
        "words": words,
        "preview": preview,
        "note": "transcript stored — run `tubeforge prompt --video-id <ID>` to build the AI gap-mining bundle",
    }))
}

/// `transcript list`: inventory of stored transcripts.
pub async fn run_list(cfg: &Config) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    let rows = db.list_transcripts().await?;
    let items: Vec<Value> = rows
        .iter()
        .map(|t| {
            json!({
                "video_id": t.video_id,
                "lang": t.lang,
                "source": t.source,
                "words": t.word_count,
                "fetched_at": t.fetched_at,
            })
        })
        .collect();
    Ok(json!({ "transcripts": items, "total": items.len() }))
}

/// `transcript clear`: wipe the transcripts table.
pub async fn run_clear(cfg: &Config) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    db.clear_transcripts().await?;
    Ok(json!({ "cleared": true }))
}
