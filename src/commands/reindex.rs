//! `reindex` (LLD §4.1, §9.2): rebuild the tantivy index from the `videos`
//! table. Idempotent; sets `meta.last_reindex_at`.

use serde_json::{json, Value};

use crate::config::Config;
use crate::error::TubeforgeError;
use crate::search::{rebuild, VideoDoc};
use crate::storage::Db;
use crate::util;

pub async fn run(cfg: &Config) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;

    let docs: Vec<VideoDoc> = db
        .all_videos()
        .await?
        .into_iter()
        .map(|v| {
            let tags: Vec<String> = serde_json::from_str(&v.tags).unwrap_or_default();
            let published_at = chrono::DateTime::parse_from_rfc3339(&v.published_at)
                .ok()
                .map(|d| d.with_timezone(&chrono::Utc).timestamp());
            VideoDoc {
                video_id: v.video_id,
                channel_id: v.channel_id,
                title: v.title,
                description: v.description,
                tags,
                published_at,
            }
        })
        .collect();

    let dir = cfg.index_dir();
    let n = rebuild(&dir, &docs)?;

    let at = util::now_rfc3339();
    db.meta_set("last_reindex_at", &at).await?;

    Ok(json!({
        "docs": n,
        "index_dir": dir.to_string_lossy(),
        "last_reindex_at": at,
    }))
}
