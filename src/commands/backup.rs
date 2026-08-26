//! `backup`: VACUUM INTO snapshot + integrity_check + retention prune.

use std::path::PathBuf;

use serde_json::{json, Value};

use crate::config::Config;
use crate::error::TubeforgeError;
use crate::storage::backup;
use crate::storage::Db;

pub async fn run(cfg: &Config, to: Option<PathBuf>) -> Result<Value, TubeforgeError> {
    let dir = to.unwrap_or_else(|| cfg.backup_dir.clone());
    let db = Db::open(&cfg.db_path).await?;

    // Backup guard: integrity failure -> exit 5, never back up a corrupt db.
    db.integrity_check().await?;

    let snapshot = backup::backup(&db, &dir, cfg.backup_keep).await?;
    let size = match std::fs::metadata(&snapshot) {
        Ok(m) => m.len(),
        Err(e) => {
            tracing::warn!(
                snapshot = %snapshot.display(),
                error = %e,
                "could not stat backup snapshot; reporting size 0"
            );
            0
        }
    };

    Ok(json!({
        "snapshot": snapshot.to_string_lossy(),
        "size_bytes": size,
        "keep": cfg.backup_keep,
    }))
}
