//! Keyword rank tracking (LLD §8.3): `keywords check` snapshots the corpus
//! ranking per tracked keyword into `keyword_rankings` (position NULL when
//! below threshold); `keywords report` renders trends across snapshots with
//! deltas computed in Rust (lag/lead are unavailable in Turso 0.7.2).

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};

use crate::error::TubeforgeError;
use crate::search::bm25::Bm25;
use crate::search::FIELD_TITLE;
use crate::storage::db::{RankingRow, VideoRow, Db};
use crate::util;

/// Rank positions at or below this are tracked; deeper ranks snapshot as
/// position NULL (LLD §8.3 "below threshold"). Baked, documented.
pub const RANK_THRESHOLD: i64 = 10;

/// `keywords check`: for every tracked keyword, run BM25 over the corpus,
/// take the user's best-matching video (channel not in `competitor_ids`) and
/// record its rank among all matching videos. `video_id` NULL when the user
/// has no matching video; `position` NULL when the rank is below
/// `RANK_THRESHOLD`. Returns the number of snapshots written.
pub async fn check(
    db: &Db,
    bm25: &Bm25,
    videos: &[VideoRow],
    competitor_ids: &HashSet<String>,
) -> Result<usize, TubeforgeError> {
    let keywords = db.list_keywords().await?;
    if keywords.is_empty() {
        return Ok(0);
    }
    let videos_by_id: HashMap<&str, &VideoRow> =
        videos.iter().map(|v| (v.video_id.as_str(), v)).collect();
    let checked_at = util::now_rfc3339();

    let mut snapshots = 0;
    for row in keywords {
        let kw = row.keyword;
        let matches = bm25.matches(FIELD_TITLE, &kw);
        // Top own match: the first corpus hit whose channel is not a
        // competitor. Videos without a channel are unattributed — not own.
        let mut top_own: Option<(usize, String)> = None;
        for (idx, (vid, _)) in matches.iter().enumerate() {
            let channel = videos_by_id.get(vid.as_str()).and_then(|v| v.channel_id.as_deref());
            let own = channel.is_some_and(|c| !competitor_ids.contains(c));
            if own {
                top_own = Some((idx, vid.clone()));
                break;
            }
        }
        let (video_id, position) = match top_own {
            None => (None, None),
            Some((idx, vid)) => {
                let pos = idx as i64 + 1;
                (Some(vid), (pos <= RANK_THRESHOLD).then_some(pos))
            }
        };
        // Denormalized snapshot of the winning video's topic labels (derived
        // at read time from topic_categories URLs; C2 dimension).
        let topics: Option<String> = video_id.as_deref().and_then(|vid| videos_by_id.get(vid)).map(|v| {
            let urls: Vec<String> = serde_json::from_str(&v.topic_categories).unwrap_or_default();
            let labels = crate::scoring::geo::topic_labels(&urls);
            serde_json::to_string(&labels).unwrap_or_else(|_| "[]".to_string())
        });
        db.upsert_ranking(&kw, &checked_at, video_id.as_deref(), position, topics.as_deref())
            .await?;
        snapshots += 1;
    }
    Ok(snapshots)
}

/// Trend rows per keyword across snapshots: latest position, previous
/// position, and the Rust-computed delta (LLD §8.3). Pure function over the
/// ordered snapshot list (testable without a database).
pub fn trend_rows(rankings: &[RankingRow]) -> Vec<Value> {
    let mut by_keyword: Vec<(String, Vec<&RankingRow>)> = Vec::new();
    for r in rankings {
        match by_keyword.last_mut() {
            Some((kw, rows)) if *kw == r.keyword => rows.push(r),
            _ => by_keyword.push((r.keyword.clone(), vec![r])),
        }
    }

    by_keyword
        .into_iter()
        .map(|(keyword, rows)| {
            let latest = rows.last().copied().expect("non-empty group");
            // Previous snapshot only exists with ≥2 snapshots (a lone
            // snapshot has no trend).
            let previous = if rows.len() >= 2 {
                rows.get(rows.len() - 2).copied()
            } else {
                None
            };
            let delta = match (previous.and_then(|p| p.position), latest.position) {
                (Some(prev), Some(cur)) => Some(cur - prev),
                _ => None,
            };
            let snapshots: Vec<Value> = rows
                .iter()
                .map(|r| {
                    json!({
                        "checked_at": r.checked_at,
                        "video_id": r.video_id,
                        "position": r.position,
                        "topics": r.topics.as_deref()
                            .and_then(|t| serde_json::from_str(t).ok())
                            .unwrap_or(Value::Null),
                    })
                })
                .collect();
            json!({
                "keyword": keyword,
                "snapshots": snapshots,
                "latest_position": latest.position,
                "previous_position": previous.and_then(|p| p.position),
                "delta": delta,
            })
        })
        .collect()
}

/// `keywords report`: latest trend data per keyword (deltas in Rust).
pub async fn report(db: &Db) -> Result<Value, TubeforgeError> {
    let rankings = db.list_rankings().await?;
    Ok(json!({ "keywords": trend_rows(&rankings) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(keyword: &str, checked_at: &str, video: Option<&str>, pos: Option<i64>) -> RankingRow {
        RankingRow {
            keyword: keyword.to_string(),
            checked_at: checked_at.to_string(),
            video_id: video.map(|s| s.to_string()),
            position: pos,
            topics: None,
        }
    }

    #[test]
    fn trend_carries_topic_labels() {
        let rows = vec![
            RankingRow {
                keyword: "rust".to_string(),
                checked_at: "2026-08-01T00:00:00Z".to_string(),
                video_id: Some("v1".to_string()),
                position: Some(1),
                topics: Some(r#"["Artificial intelligence"]"#.to_string()),
            },
            RankingRow {
                keyword: "rust".to_string(),
                checked_at: "2026-08-02T00:00:00Z".to_string(),
                video_id: Some("v1".to_string()),
                position: Some(2),
                topics: None,
            },
        ];
        let trends = trend_rows(&rows);
        let snap = &trends[0]["snapshots"];
        assert_eq!(snap[0]["topics"], json!(["Artificial intelligence"]));
        assert_eq!(snap[1]["topics"], Value::Null, "no topics recorded → null");
    }

    /// Delta math over snapshots: improvement (7→3) shows delta −4; a first
    /// snapshot has no previous position; an unranked snapshot (NULL) yields
    /// no delta.
    #[test]
    fn trend_delta_math() {
        let rows = vec![
            row("rust", "2026-08-01T00:00:00Z", Some("v1"), Some(7)),
            row("rust", "2026-08-02T00:00:00Z", Some("v1"), Some(3)),
            row("sql", "2026-08-01T00:00:00Z", Some("v2"), Some(5)),
            row("sql", "2026-08-02T00:00:00Z", None, None), // unranked
            row("db", "2026-08-02T00:00:00Z", Some("v3"), Some(2)),
        ];
        let trends = trend_rows(&rows);
        assert_eq!(trends.len(), 3);
        let t = |kw: &str| {
            trends
                .iter()
                .find(|t| t["keyword"] == kw)
                .expect("row")
                .clone()
        };
        assert_eq!(t("rust")["delta"], -4);
        assert_eq!(t("rust")["latest_position"], 3);
        assert_eq!(t("rust")["previous_position"], 7);
        assert_eq!(t("sql")["delta"], Value::Null, "NULL position → no delta");
        assert_eq!(t("db")["previous_position"], Value::Null, "first snapshot");
        assert_eq!(t("db")["delta"], Value::Null);
        assert_eq!(t("db")["latest_position"], 2);
    }

    #[test]
    fn trend_groups_by_keyword_in_order() {
        let rows = vec![
            row("a", "2026-08-01T00:00:00Z", Some("v1"), Some(1)),
            row("a", "2026-08-02T00:00:00Z", Some("v1"), Some(2)),
            row("b", "2026-08-01T00:00:00Z", Some("v2"), Some(3)),
        ];
        let trends = trend_rows(&rows);
        assert_eq!(trends[0]["keyword"], "a");
        assert_eq!(trends[0]["snapshots"].as_array().unwrap().len(), 2);
        assert_eq!(trends[1]["keyword"], "b");
    }
}
