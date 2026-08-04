//! `scorecard` (LLD §4.1, §8.4): per-channel competitor comparison vs the
//! median of the set — views growth proxy, title patterns, tag overlap,
//! PageRank centrality, SEO score distribution.

use serde_json::Value;

use crate::analytics::reports;
use crate::config::Config;
use crate::error::TubeforgeError;
use crate::storage::Db;

pub async fn run(cfg: &Config, channels: &[String]) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    reports::scorecard(&db, channels).await
}
