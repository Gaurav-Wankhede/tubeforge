//! Backup service (LLD §9.1): VACUUM INTO snapshot + integrity_check on the
//! snapshot + retention prune. Auto-run before every batch ingest (Phase 1).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::db::Db;
use crate::error::TubeforgeError;

/// Snapshot file prefix.
pub const SNAPSHOT_PREFIX: &str = "tubeforge-";

/// Run the locked backup policy (LLD §9.1):
/// 1. `VACUUM INTO <dir>/tubeforge-<ts>.db` (consistent single-file snapshot)
/// 2. `integrity_check` on the snapshot (fail -> Integrity, exit 5)
/// 3. prune to keep last N (TUBEFORGE_BACKUP_KEEP, default 10)
/// 4. persist `meta.last_backup_at`
///
/// Returns the snapshot path.
pub async fn backup(db: &Db, dir: &Path, keep: usize) -> Result<PathBuf, TubeforgeError> {
    std::fs::create_dir_all(dir).map_err(|e| TubeforgeError::Storage {
        code: "IO".to_string(),
        message: format!("create backup dir {}: {e}", dir.display()),
    })?;

    let ts = iso_timestamp_utc();
    let snapshot = dir.join(format!("{SNAPSHOT_PREFIX}{ts}.db"));

    db.vacuum_into(&snapshot).await?;

    // Integrity-check the snapshot itself, not just the source (exit 5 path).
    let snap_db = Db::open(&snapshot).await?;
    snap_db.integrity_check().await.map_err(|e| match e {
        TubeforgeError::Integrity { detail } => TubeforgeError::Integrity {
            detail: format!("snapshot {}: {detail}", snapshot.display()),
        },
        other => other,
    })?;

    let pruned = prune(dir, keep)?;

    db.meta_set("last_backup_at", &ts).await?;

    tracing::info!(
        snapshot = %snapshot.display(),
        kept = keep,
        pruned,
        "backup complete"
    );
    Ok(snapshot)
}

/// Remove snapshots beyond the keep-last-N policy. Returns count removed.
/// Snapshot names are lexicographically chronological (ISO timestamps).
/// Engine companion files (`-wal`, `-shm`) created when turso opens a
/// snapshot for integrity checking are pruned together with their snapshot.
pub fn prune(dir: &Path, keep: usize) -> Result<usize, TubeforgeError> {
    let mut snapshots: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| TubeforgeError::Storage {
            code: "IO".to_string(),
            message: format!("read backup dir {}: {e}", dir.display()),
        })?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(SNAPSHOT_PREFIX) && n.ends_with(".db"))
        })
        .collect();

    // Oldest first, so we drop from the front beyond `keep`.
    snapshots.sort();
    let excess = snapshots.len().saturating_sub(keep);
    let mut pruned = 0;
    for old in snapshots.into_iter().take(excess) {
        remove_snapshot(&old)?;
        pruned += 1;
    }
    Ok(pruned)
}

/// Remove a snapshot file plus any engine companion files it spawned.
fn remove_snapshot(snapshot: &Path) -> Result<(), TubeforgeError> {
    let base = snapshot.as_os_str().to_owned();
    std::fs::remove_file(snapshot).map_err(|e| TubeforgeError::Storage {
        code: "IO".to_string(),
        message: format!("prune {}: {e}", snapshot.display()),
    })?;
    for suffix in ["-wal", "-shm"] {
        let companion = PathBuf::from(format!("{}{}", base.to_string_lossy(), suffix));
        let _ = std::fs::remove_file(companion);
    }
    // tfdb snapshots mirror the checkpoint to `<snapshot>.dat` (so `Db::open`
    // round-trips) plus its WAL companions; clean those up too.
    for suffix in [".dat", ".dat-wal", ".dat-shm"] {
        let companion = PathBuf::from(format!("{}{}", base.to_string_lossy(), suffix));
        let _ = std::fs::remove_file(companion);
    }
    Ok(())
}

/// Current UTC time as `YYYYMMDD-HHMMSS-mmm` (lexicographically chronological,
/// millisecond precision so rapid successive snapshots never collide; no
/// chrono dependency in the Phase 0 dependency set).
pub fn iso_timestamp_utc() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() as i64;
    let millis = now.subsec_millis();
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}{mo:02}{d:02}-{h:02}{m:02}{s:02}-{millis:03}")
}

/// Howard Hinnant's civil-from-days algorithm.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_date_sanity() {
        let (y, m, d) = civil_from_days(0);
        assert_eq!((y, m, d), (1970, 1, 1));
        let (y, m, d) = civil_from_days(20_635); // 2026-07-01
        assert_eq!((y, m, d), (2026, 7, 1));
    }

    #[test]
    fn timestamp_format() {
        let ts = iso_timestamp_utc();
        assert_eq!(ts.len(), 19); // YYYYMMDD-HHMMSS-mmm
        assert_eq!(&ts[8..9], "-");
        assert!(ts
            .as_bytes()
            .iter()
            .all(|b| b.is_ascii_digit() || *b == b'-'));
    }
}
