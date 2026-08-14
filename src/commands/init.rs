//! `init`: create data root, .env scaffold, DB + migrations, test open.

use serde_json::{json, Value};

use crate::config::Config;
use crate::error::TubeforgeError;
use crate::storage::Db;

/// `.env.example` content is embedded from the repo-root file so `init` can
/// scaffold it at the data root even when run from another directory.
const ENV_EXAMPLE: &str = include_str!("../../.env.example");

pub async fn run(cfg: &Config) -> Result<Value, TubeforgeError> {
    std::fs::create_dir_all(&cfg.data_dir).map_err(|e| {
        TubeforgeError::Config(format!(
            "cannot create data dir {}: {e}",
            cfg.data_dir.display()
        ))
    })?;
    std::fs::create_dir_all(&cfg.backup_dir).map_err(|e| {
        TubeforgeError::Config(format!(
            "cannot create backup dir {}: {e}",
            cfg.backup_dir.display()
        ))
    })?;

    let env_path = cfg.data_dir.join(".env.example");
    if !env_path.exists() {
        std::fs::write(&env_path, ENV_EXAMPLE).map_err(|e| {
            TubeforgeError::Config(format!("cannot write {}: {e}", env_path.display()))
        })?;
        tracing::info!(path = %env_path.display(), "wrote .env.example");
    } else {
        tracing::info!(path = %env_path.display(), ".env.example exists, leaving unchanged");
    }

    let db = Db::open(&cfg.db_path).await?;
    let journal_mode = db.journal_mode().await?;
    let integrity = db.integrity_check().await.is_ok();

    // Index dir is created lazily by ingest/reindex, but init pre-creates it
    // so the data-root layout is complete (LLD §9 data root).
    std::fs::create_dir_all(cfg.index_dir()).map_err(|e| {
        TubeforgeError::Config(format!(
            "cannot create index dir {}: {e}",
            cfg.index_dir().display()
        ))
    })?;

    tracing::info!(
        db = %cfg.db_path.display(),
        journal_mode = %journal_mode,
        "database initialized"
    );

    Ok(json!({
        "db_path": cfg.db_path.to_string_lossy(),
        "journal_mode": journal_mode,
        "integrity": if integrity { "ok" } else { "FAILED" },
        "env_example": env_path.to_string_lossy(),
        "data_dir": cfg.data_dir.to_string_lossy(),
        "backup_dir": cfg.backup_dir.to_string_lossy(),
        "index_dir": cfg.index_dir().to_string_lossy(),
    }))
}
