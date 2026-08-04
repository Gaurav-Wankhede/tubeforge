//! `quota`: show the YouTube Data API usage ledger with midnight-PT rollover
//! (LLD §5.4). Reads the meta table; renders used/limit/date/warn threshold.

use serde_json::{json, Value};

use crate::config::Config;
use crate::error::TubeforgeError;
use crate::fetch::quota::{self, DAILY_LIMIT};
use crate::storage::Db;

pub async fn run(cfg: &Config) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    let (used, date) = quota::used(&db).await?;

    Ok(json!({
        "videos_list": {
            "used": used,
            "daily_limit": DAILY_LIMIT,
            "date": date,
        },
        "warn_at_percent": cfg.quota_warn_at,
    }))
}
