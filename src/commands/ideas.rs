//! `ideas` (LLD §4.1, §8.2): generate + persist the Next Idea pool, mark
//! statuses (draft|saved|discarded), render ranked by score.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::analytics::ideas;
use crate::config::Config;
use crate::error::TubeforgeError;
use crate::scoring::weights::Weights;
use crate::search::bm25::Bm25;
use crate::search::open_or_create;
use crate::storage::db::{VideoRow, Db};

/// Valid idea statuses (LLD §3.1 default `draft`).
pub const STATUSES: [&str; 3] = ["draft", "saved", "discarded"];

pub async fn run(
    cfg: &Config,
    limit: usize,
    niche: Option<&str>,
    status: Option<&str>,
) -> Result<Value, TubeforgeError> {
    if let Some(s) = status {
        if !STATUSES.contains(&s) {
            return Err(TubeforgeError::Usage(format!(
                "invalid --status {s:?} (expected {})",
                STATUSES.join("|")
            )));
        }
    }

    let db = Db::open(&cfg.db_path).await?;
    let videos = db.all_videos().await?;
    if videos.is_empty() {
        return Ok(json!({
            "ideas": [],
            "note": "no videos in database — run `tubeforge ingest` first",
        }));
    }

    let index = open_or_create(&cfg.index_dir())?;
    let bm25 = Bm25::open(index)?;
    let weights = Weights::from_env()?;

    let mut pool = ideas::generate(&db, &bm25, &videos, &weights, niche, limit).await?;

    // `--status X` marks the freshly generated pool (LLD §8.2 status marking)
    // and filters the render to that status.
    if let Some(s) = status {
        let ids: Vec<i64> = pool.iter().map(|i| i.idea_id).collect();
        db.set_idea_statuses(&ids, s).await?;
        for i in &mut pool {
            i.status = s.to_string();
        }
    }

    let videos_by_id: HashMap<&str, &VideoRow> =
        videos.iter().map(|v| (v.video_id.as_str(), v)).collect();

    let mut rows: Vec<Value> = Vec::new();
    for i in &pool {
        let mut row = json!({
            "idea_id": i.idea_id,
            "title": i.title_suggestion,
            "score": i.score,
            "status": i.status,
            "source_video": i.source_video,
            "rationale": i.rationale,
        });
        // A3: render the category display name (raw id when unknown).
        let category = i
            .source_video
            .as_ref()
            .and_then(|vid| videos_by_id.get(vid.as_str()))
            .and_then(|v| category_of(Some(v)));
        if let Some(cat) = category {
            row["category"] = json!(cat);
        }
        rows.push(row);
    }

    Ok(json!({
        "ideas": rows,
        "limit": limit,
        "niche": niche,
        "status": status,
    }))
}

/// The source video's category rendered by display name (A3); unknown or
/// absent category ids → None.
fn category_of(video: Option<&VideoRow>) -> Option<String> {
    video
        .and_then(|v| v.category_id.as_deref())
        .map(|cid| crate::categories::category_name(cid).unwrap_or(cid).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_of_renders_name_or_raw_id() {
        let mk = |category: Option<&str>| VideoRow {
            category_id: category.map(String::from),
            ..Default::default()
        };
        assert_eq!(category_of(Some(&mk(Some("42")))), Some("Shorts".to_string()));
        assert_eq!(
            category_of(Some(&mk(Some("28")))),
            Some("Science & Technology".to_string())
        );
        assert_eq!(category_of(Some(&mk(Some("999")))), Some("999".to_string()));
        assert_eq!(category_of(Some(&mk(None))), None);
        assert_eq!(category_of(None), None);
    }
}
