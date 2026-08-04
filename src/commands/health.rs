//! `health` (LLD §4.1, §8.4): row counts, last ingest, quota state,
//! integrity_check result, stale channels, index freshness.

use serde_json::Value;

use crate::analytics::reports::{self, DEFAULT_STALE_DAYS};
use crate::config::Config;
use crate::error::TubeforgeError;
use crate::storage::Db;

pub async fn run(cfg: &Config) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    let stale_days: u32 = match std::env::var("TUBEFORGE_STALE_DAYS") {
        Ok(v) => v
            .parse()
            .map_err(|_| TubeforgeError::Config(format!("TUBEFORGE_STALE_DAYS not a number: {v}")))?,
        Err(_) => DEFAULT_STALE_DAYS,
    };
    reports::health(&db, stale_days).await
}
