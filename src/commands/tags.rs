//! `tags` (LLD §8.3 extension): `backfill` populates the normalized tag
//! tables (`tags`, `video_tags`, `competitor_tags`) from stored video rows;
//! `analyze` aggregates per-channel tag stats into `competitor_tags`.

use serde_json::{json, Value};

use crate::analytics::tags;
use crate::config::Config;
use crate::error::TubeforgeError;
use crate::storage::Db;

/// `tags backfill`: populate tag tables from existing video data.
pub async fn run_backfill(cfg: &Config) -> Result<Value, TubeforgeError> {
    let mut db = Db::open(&cfg.db_path).await?;
    let backfilled = tags::backfill_tags(&mut db).await?;
    let total = db.count_tags().await?;
    Ok(json!({
        "backfilled_videos": backfilled,
        "total_tags": total,
    }))
}

/// `tags analyze`: aggregate per-channel tag stats into competitor_tags
/// (the table `/api/tags/gaps` reads). Run after backfill.
pub async fn run_analyze(cfg: &Config) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    let upserted = tags::analyze_competitors(&db).await?;
    Ok(json!({
        "competitor_tag_rows": upserted,
    }))
}
