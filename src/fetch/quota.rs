//! Per-endpoint quota ledger (LLD §5.4), persisted in `meta`.
//!
//! `videos.list` costs 1 unit per call with a 10,000/day budget, reset at
//! midnight America/Los_Angeles (the YouTube Data API resets at midnight
//! Pacific Time).

use chrono::Utc;
use chrono_tz::America::Los_Angeles;

use crate::error::TubeforgeError;
use crate::storage::Db;

/// Daily budget for `videos.list` (LLD §5.3).
pub const DAILY_LIMIT: u64 = 10_000;

const KEY_USED: &str = "quota_videos_list_used";
const KEY_DATE: &str = "quota_videos_list_date";

/// Today's quota bucket label in America/Los_Angeles (YYYY-MM-DD).
pub fn today_pt() -> String {
    Utc::now()
        .with_timezone(&Los_Angeles)
        .format("%Y-%m-%d")
        .to_string()
}

/// Current usage with rollover: if the stored date is not today, usage is 0
/// (the bucket reset at midnight PT).
pub async fn used(db: &Db) -> Result<(u64, String), TubeforgeError> {
    let today = today_pt();
    let stored_date = db.meta_get(KEY_DATE).await?;
    let stored_used = db
        .meta_get(KEY_USED)
        .await?
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let effective = if stored_date.as_deref() == Some(today.as_str()) {
        stored_used
    } else {
        0
    };
    Ok((effective, today))
}

/// Record `calls` units for `videos.list` today (resets the bucket first when
/// the stored date is stale).
pub async fn record_videos_list_calls(db: &Db, calls: u64) -> Result<(), TubeforgeError> {
    let (used, today) = used(db).await?;
    db.meta_set(KEY_USED, &(used.saturating_add(calls)).to_string())
        .await?;
    db.meta_set(KEY_DATE, &today).await?;
    Ok(())
}

/// Record `calls` units for `commentThreads.list` today. Shares the same
/// daily bucket as `videos.list` (one 10,000/day budget across the API —
/// LLD §5.4) so the ledger stays a single number.
pub async fn record_comment_threads_calls(db: &Db, calls: u64) -> Result<(), TubeforgeError> {
    let (used, today) = used(db).await?;
    db.meta_set(KEY_USED, &(used.saturating_add(calls)).to_string())
        .await?;
    db.meta_set(KEY_DATE, &today).await?;
    Ok(())
}

/// Pre-flight check (LLD §5.4): would `projected` calls fit in today's
/// remaining budget? `warn` is true at/above the TUBEFORGE_QUOTA_WARN_AT
/// percent of the daily limit.
pub struct Preflight {
    pub used: u64,
    pub projected: u64,
    pub remaining: u64,
    pub warn: bool,
}

pub async fn preflight(db: &Db, projected: u64, warn_at: u64) -> Result<Preflight, TubeforgeError> {
    let (used, _) = used(db).await?;
    let remaining = DAILY_LIMIT.saturating_sub(used);
    let after = used.saturating_add(projected);
    let pct = if after >= DAILY_LIMIT {
        100
    } else {
        (after * 100) / DAILY_LIMIT
    };
    Ok(Preflight {
        used,
        projected,
        remaining,
        warn: pct >= warn_at.min(100),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn today_pt_format() {
        let t = today_pt();
        assert_eq!(t.len(), 10);
        assert_eq!(&t[4..5], "-");
        assert_eq!(&t[7..8], "-");
    }
}
