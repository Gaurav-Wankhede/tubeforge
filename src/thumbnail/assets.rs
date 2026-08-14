//! Per-render temporary assets directory (PRD §5.7).
//!
//! Raw assets (source thumbnails, fonts, ...) are written under
//! `<TUBEFORGE_DATA_DIR>/assets/<render-id>/` and MUST be deleted as soon as
//! generation finishes — success AND failure. An [`AssetDir`] guard removes
//! the directory on drop unless `keep` is set (the `--keep-assets` debug
//! flag; never on by default).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{storage_err, TubeforgeError};
use crate::util::batch_id;

/// Monotonic per-process counter so two renders in the same second get
/// distinct directories.
static RENDER_SEQ: AtomicU64 = AtomicU64::new(0);

/// RAII guard for the raw-assets directory of a single render.
///
/// The directory is removed when the guard drops (success or error path);
/// only `keep = true` (explicit debug opt-in) leaves it behind.
pub struct AssetDir {
    path: PathBuf,
    keep: bool,
}

impl AssetDir {
    /// Create `<data>/assets/<batch>-<pid>-<seq>/` for a new render.
    pub fn create(data_dir: &Path, keep: bool) -> Result<AssetDir, TubeforgeError> {
        let seq = RENDER_SEQ.fetch_add(1, Ordering::Relaxed);
        let path =
            data_dir
                .join("assets")
                .join(format!("{}-{}-{}", batch_id(), std::process::id(), seq));
        std::fs::create_dir_all(&path).map_err(|e| {
            storage_err(
                "ASSETS_CREATE",
                format!("cannot create assets dir {}: {e}", path.display()),
            )
        })?;
        Ok(AssetDir { path, keep })
    }

    /// The assets directory of this render (write raw assets into it).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the directory survives the guard (`--keep-assets`, debug only).
    pub fn keep(&self) -> bool {
        self.keep
    }
}

impl Drop for AssetDir {
    fn drop(&mut self) {
        if self.keep {
            tracing::debug!(dir = %self.path.display(), "asset dir kept (--keep-assets, debug)");
            return;
        }
        match std::fs::remove_dir_all(&self.path) {
            Ok(()) => {
                // Tidy up the now-empty `<data>/assets` parent as well, so a
                // successful render leaves no trace.
                if let Some(parent) = self.path.parent() {
                    let _ = std::fs::remove_dir(parent);
                }
                tracing::debug!(dir = %self.path.display(), "raw assets deleted");
            }
            // Already gone (e.g. the renderer cleaned up first): fine.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            // Cleanup must never mask the render result, so failures are
            // logged, not propagated.
            Err(e) => tracing::warn!(dir = %self.path.display(), "assets cleanup failed: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The success path: guard drops → directory (with contents) is gone.
    #[test]
    fn guard_removes_dir_after_successful_render() {
        let root = tempfile::tempdir().expect("tempdir");
        let dir = AssetDir::create(root.path(), false).expect("create");
        std::fs::write(dir.path().join("raw_asset.bin"), b"x").expect("write asset");
        let path = dir.path().to_path_buf();
        assert!(path.exists());

        drop(dir); // render succeeded, scope ends

        assert!(!path.exists(), "assets dir must be deleted on success");
        assert_eq!(
            std::fs::read_dir(root.path()).expect("read root").count(),
            0
        );
    }

    /// The error path: the guard is alive when the render fails; the early
    /// return still removes the dir via Drop.
    #[test]
    fn guard_removes_dir_when_render_errors() {
        let root = tempfile::tempdir().expect("tempdir");
        let result = simulate_failed_render(root.path());
        assert!(result.is_err());

        let assets = root.path().join("assets");
        assert!(!assets.exists(), "assets dir must be deleted on error");
    }

    /// `--keep-assets` (debug only) is the sole exception to cleanup.
    #[test]
    fn guard_keeps_dir_with_keep_flag() {
        let root = tempfile::tempdir().expect("tempdir");
        let dir = AssetDir::create(root.path(), true).expect("create");
        let path = dir.path().to_path_buf();
        drop(dir);
        assert!(path.exists(), "keep=true must leave the dir behind");
    }

    /// Render flow that fails after assets were created — the guard must
    /// clean up on this early-return path too.
    fn simulate_failed_render(data_dir: &Path) -> Result<(), TubeforgeError> {
        let guard = AssetDir::create(data_dir, false)?;
        std::fs::write(guard.path().join("raw_asset.bin"), b"x")
            .map_err(|e| storage_err("IO", e.to_string()))?;
        Err(storage_err("RENDER", "simulated render failure"))
    }
}
