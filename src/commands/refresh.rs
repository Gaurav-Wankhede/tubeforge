//! `refresh [--channel <id>...] [--no-backup]` (LLD §4.1): re-fetch known
//! channels with ETag caching — 304 → skipped, no writes, no snapshot.

use serde_json::{json, Value};

use crate::commands::ingest::summary_json;
use crate::config::Config;
use crate::error::TubeforgeError;
use crate::fetch::FetchClients;
use crate::ingest::{self, IngestOptions};
use crate::storage::Db;

pub async fn run(
    cfg: &Config,
    channels: &[String],
    no_backup: bool,
) -> Result<Value, TubeforgeError> {
    let clients = FetchClients::new()?;
    let mut db = Db::open(&cfg.db_path).await?;
    let opts = IngestOptions {
        use_api: false,
        no_backup,
    };
    let summary = ingest::refresh_channels(cfg, &clients, &mut db, channels, &opts).await?;
    let mut data = summary_json(&summary);
    data["command"] = json!("refresh");
    Ok(data)
}
