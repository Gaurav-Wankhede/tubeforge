//! `videos` (Phase 6.6): video library maintenance.
//!
//! `dedupe`: collapse duplicate videos — rows sharing (channel_id, title)
//! that are the same underlying upload with different video_ids (a
//! re-upload/mirror, or a SERP listing with a variant id) merge into ONE
//! record. The richest row wins (non-empty description, most views);
//! scores/tags/heatmap/transcripts/comments/keyword_rankings/ideas are
//! repointed to the winner, losers deleted. Idempotent.

use serde_json::{json, Value};

use crate::config::Config;
use crate::error::TubeforgeError;
use crate::storage::Db;

/// `videos dedupe`: merge duplicate videos into one record each.
pub async fn run_dedupe(cfg: &Config) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    let (merged, deleted) = crate::storage::db::dedupe_videos(&db).await?;
    let remaining = db.count("SELECT count(*) FROM videos").await.unwrap_or(0);
    Ok(json!({
        "groups_merged": merged,
        "rows_deleted": deleted,
        "videos_remaining": remaining,
        "note": "duplicate videos (same channel + title) collapsed to one record each; \
                 scores/tags/transcripts/comments repointed to the winner",
    }))
}
