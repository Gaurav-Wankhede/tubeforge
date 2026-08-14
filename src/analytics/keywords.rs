//! Keyword rank tracking (LLD §8.3): `keywords check` snapshots the corpus
//! ranking per tracked keyword into `keyword_rankings` (position NULL when
//! below threshold); `keywords report` renders trends across snapshots with
//! deltas computed in Rust (lag/lead are unavailable in Turso 0.7.2).

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Value};

use crate::error::TubeforgeError;
use crate::search::bm25::Bm25;
use crate::search::FIELD_TITLE;
use crate::storage::db::{Db, RankingRow, VideoRow};
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
    let mut checked_at = util::now_rfc3339();
    // A same-second recheck would silently overwrite the earlier snapshot
    // (PK keyword+checked_at). Bump until the timestamp is free so history is
    // never lost.
    while db.ranking_count_at(&checked_at).await? > 0 {
        checked_at = bump_second(&checked_at);
    }

    let mut snapshots = 0;
    for row in keywords {
        let kw = row.keyword;
        let matches = bm25.matches(FIELD_TITLE, &kw);
        // Top own match: the first corpus hit whose channel is not a
        // competitor. Videos without a channel are unattributed — not own.
        let mut top_own: Option<(usize, String)> = None;
        for (idx, (vid, _)) in matches.iter().enumerate() {
            let channel = videos_by_id
                .get(vid.as_str())
                .and_then(|v| v.channel_id.as_deref());
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
        let topics: Option<String> = video_id
            .as_deref()
            .and_then(|vid| videos_by_id.get(vid))
            .map(|v| {
                let urls: Vec<String> =
                    serde_json::from_str(&v.topic_categories).unwrap_or_default();
                let labels = crate::scoring::geo::topic_labels(&urls);
                serde_json::to_string(&labels).unwrap_or_else(|_| "[]".to_string())
            });
        db.upsert_ranking(
            &kw,
            &checked_at,
            video_id.as_deref(),
            position,
            topics.as_deref(),
        )
        .await?;
        snapshots += 1;
    }
    Ok(snapshots)
}

/// Advance an RFC3339 seconds-precision timestamp by one second. Used to
/// avoid overwriting an existing keyword snapshot within the same second.
fn bump_second(ts: &str) -> String {
    DateTime::parse_from_rfc3339(ts)
        .map(|d| d.with_timezone(&Utc) + chrono::Duration::seconds(1))
        .unwrap_or_else(|_| Utc::now())
        .to_rfc3339_opts(SecondsFormat::Secs, true)
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
                            .and_then(|t| serde_json::from_str::<Value>(t).ok()),
                    })
                })
                .collect();
            json!({
                "keyword": keyword,
                "latest_position": latest.position,
                "previous_position": previous.and_then(|p| p.position),
                "delta": delta,
                "snapshots": snapshots,
            })
        })
        .collect()
}

/// `keywords report`: latest trend rows per keyword with Rust-computed
/// deltas. Returns `{"keywords": [...]}`.
pub async fn report(db: &Db) -> Result<Value, TubeforgeError> {
    let rankings = db.list_rankings().await?;
    let trends = trend_rows(&rankings);
    Ok(json!({ "keywords": trends }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(keyword: &str, checked_at: &str, position: Option<i64>) -> RankingRow {
        RankingRow {
            keyword: keyword.to_string(),
            checked_at: checked_at.to_string(),
            video_id: None,
            position,
            topics: None,
        }
    }

    #[test]
    fn trend_rows_groups_and_deltas() {
        let rows = vec![
            r("rust", "2026-01-01T00:00:00Z", Some(5)),
            r("rust", "2026-01-02T00:00:00Z", Some(3)),
            r("go", "2026-01-02T00:00:00Z", Some(8)),
        ];
        let trends = trend_rows(&rows);
        assert_eq!(trends.len(), 2);
        assert_eq!(trends[0]["keyword"], "rust");
        assert_eq!(trends[0]["delta"], -2);
        assert_eq!(trends[0]["snapshots"].as_array().unwrap().len(), 2);
        assert_eq!(trends[1]["keyword"], "go");
        assert_eq!(trends[1]["delta"], Value::Null);
    }

    #[test]
    fn bump_second_advances_rfc3339() {
        assert_eq!(bump_second("2026-01-01T00:00:00Z"), "2026-01-01T00:00:01Z");
    }
}
