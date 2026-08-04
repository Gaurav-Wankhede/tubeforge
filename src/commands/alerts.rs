//! `alerts` (LLD §4.1, §8.4): evaluate the alert rules, insert new alerts,
//! render the list; `alerts list` (no evaluation), `alerts clear`,
//! `--mark-read`.

use serde_json::{json, Value};

use crate::analytics::reports::{self, DEFAULT_STALE_DAYS};
use crate::cli::AlertsAction;
use crate::config::Config;
use crate::error::TubeforgeError;
use crate::search::bm25::Bm25;
use crate::search::open_or_create;
use crate::storage::Db;

/// Render limit for the alert list.
pub const LIST_LIMIT: usize = 100;

pub async fn run(
    cfg: &Config,
    action: Option<AlertsAction>,
    mark_read: bool,
) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;

    // `alerts clear`: drop the whole table.
    if let Some(AlertsAction::Clear) = action {
        let cleared = db.clear_alerts().await?;
        return Ok(json!({ "cleared": cleared }));
    }

    // Default `alerts`: evaluate rules first, then list. `alerts list` skips
    // evaluation. The brand rule needs the index; index errors are tolerated
    // (the rule is skipped — reported below).
    let mut inserted = 0;
    let mut brand_skipped = false;
    if !matches!(action, Some(AlertsAction::List)) {
        let stale_days: u32 = match std::env::var("TUBEFORGE_STALE_DAYS") {
            Ok(v) => v.parse().map_err(|_| {
                TubeforgeError::Config(format!("TUBEFORGE_STALE_DAYS not a number: {v}"))
            })?,
            Err(_) => DEFAULT_STALE_DAYS,
        };
        let bm25 = match open_or_create(&cfg.index_dir()).and_then(Bm25::open) {
            Ok(b) => Some(b),
            Err(_) => {
                brand_skipped = true;
                None
            }
        };
        inserted = reports::evaluate_alerts(&db, cfg, stale_days, bm25.as_ref()).await?;
    }

    let marked_read = if mark_read {
        db.mark_alerts_read().await?
    } else {
        0
    };

    let alerts: Vec<Value> = db
        .list_alerts(LIST_LIMIT)
        .await?
        .into_iter()
        .map(|a| {
            json!({
                "alert_id": a.alert_id,
                "kind": a.kind,
                "severity": a.severity,
                "channel_id": a.channel_id,
                "message": a.message,
                "created_at": a.created_at,
                "read_at": a.read_at,
            })
        })
        .collect();

    Ok(json!({
        "alerts": alerts,
        "inserted": inserted,
        "marked_read": marked_read,
        "brand_rule_skipped": brand_skipped,
    }))
}
