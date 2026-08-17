//! Greedy bot history tracker: cooldown, dedup, and status reporting.

use chrono::{DateTime, Utc};
use serde_json::json;

use crate::error::TubeforgeError;
use crate::storage::db::{Db, GreedyHistoryInsert};
use crate::util;

/// Default cooldown: don't re-research the same topic within this window.
const DEFAULT_COOLDOWN_HOURS: i64 = 24;

/// Check whether `topic` is eligible for research (not in cooldown).
pub async fn is_eligible(
    db: &Db,
    topic: &str,
    cooldown_hours: Option<i64>,
) -> Result<bool, TubeforgeError> {
    let hours = cooldown_hours.unwrap_or(DEFAULT_COOLDOWN_HOURS);
    let dominated = dominated_at(db, topic).await?;
    if let Some(last_at) = dominated {
        if let Ok(last_dt) = last_at.parse::<DateTime<Utc>>() {
            let diff = Utc::now().signed_duration_since(last_dt).num_hours();
            if diff < hours {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Most recent `researched_at` for this topic (NULL if never researched).
async fn dominated_at(db: &Db, topic: &str) -> Result<Option<String>, TubeforgeError> {
    let rows = db.greedy_history_for_topic(topic).await?;
    Ok(rows.into_iter().next().map(|r| r.researched_at))
}

/// Log a completed research run into `greedy_research_history`.
pub async fn record_research(
    db: &Db,
    topic: &str,
    video_ids: &[String],
    mean_views: f64,
    source: &str,
    duration_ms: u64,
) -> Result<i64, TubeforgeError> {
    let now = util::now_rfc3339();
    let video_ids_json = serde_json::to_string(video_ids).unwrap_or_else(|_| "[]".into());
    db.insert_greedy_history(&GreedyHistoryInsert {
        topic,
        researched_at: &now,
        video_ids_json: &video_ids_json,
        video_count: video_ids.len() as i64,
        mean_views,
        source,
        duration_ms: duration_ms as i64,
    })
    .await
}

/// Log a skip / eligibility-check outcome into `greedy_topic_log`.
pub async fn log_attempt(
    db: &Db,
    topic: &str,
    status: &str,
    reason: &str,
) -> Result<i64, TubeforgeError> {
    let now = util::now_rfc3339();
    db.insert_greedy_topic_log(topic, status, reason, &now)
        .await
}

/// Aggregate stats for the `status` subcommand.
pub async fn stats(db: &Db) -> Result<serde_json::Value, TubeforgeError> {
    let total: i64 = db.greedy_history_count().await?;
    let unique: i64 = db.greedy_unique_topics().await?;
    let seeds: i64 = db.greedy_active_seed_count().await?;
    let skipped: i64 = db.greedy_topic_log_count("skipped").await?;
    let succeeded: i64 = db.greedy_topic_log_count("success").await?;
    Ok(json!({
        "total_runs": total,
        "unique_topics": unique,
        "active_seeds": seeds,
        "topics_skipped": skipped,
        "topics_succeeded": succeeded,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_cooldown_is_24h() {
        assert_eq!(DEFAULT_COOLDOWN_HOURS, 24);
    }
}
