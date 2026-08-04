//! Configuration resolution (LLD §11).
//!
//! Precedence: CLI flags > `.env` values > baked defaults.
//! `.env` sources: `--config <file>` if given (must exist), else `.env` in CWD,
//! else `~/.tubeforge/.env` (the documented data-root location, HLD §9).

use std::path::{Path, PathBuf};

use crate::error::TubeforgeError;

#[derive(Debug, Clone)]
pub struct Config {
    pub db_path: PathBuf,
    pub data_dir: PathBuf,
    pub backup_dir: PathBuf,
    pub backup_keep: usize,
    pub log_level: String,
    pub youtube_api_key: Option<String>,
    pub quota_warn_at: u64,
}

impl Config {
    /// Baked defaults (LLD §11).
    pub fn defaults() -> Self {
        let home = home_dir().unwrap_or_else(|| PathBuf::from("."));
        let data_dir = home.join(".tubeforge");
        Config {
            db_path: data_dir.join("tubeforge.db"),
            backup_dir: data_dir.join("backups"),
            data_dir,
            backup_keep: 10,
            log_level: "info".to_string(),
            youtube_api_key: None,
            quota_warn_at: 90,
        }
    }
}

/// Load `.env` per precedence rules, then resolve config.
pub fn load(cli_config: Option<&Path>, cli_db_path: Option<&Path>) -> Result<Config, TubeforgeError> {
    load_env(cli_config)?;

    let mut cfg = Config::defaults();

    if let Some(path) = cli_db_path {
        cfg.db_path = path.to_path_buf();
    } else if let Ok(v) = std::env::var("TUBEFORGE_DB_PATH") {
        cfg.db_path = expand_tilde(&v);
    }

    if let Ok(v) = std::env::var("TUBEFORGE_DATA_DIR") {
        cfg.data_dir = expand_tilde(&v);
    }

    if let Ok(v) = std::env::var("TUBEFORGE_BACKUP_DIR") {
        cfg.backup_dir = expand_tilde(&v);
    } else {
        // Default: <data>/backups (LLD §11).
        cfg.backup_dir = cfg.data_dir.join("backups");
    }

    if let Ok(v) = std::env::var("TUBEFORGE_BACKUP_KEEP") {
        cfg.backup_keep = v
            .parse()
            .map_err(|_| TubeforgeError::Config(format!("TUBEFORGE_BACKUP_KEEP not a number: {v}")))?;
    }

    if let Ok(v) = std::env::var("LOG_LEVEL") {
        cfg.log_level = v;
    }

    cfg.youtube_api_key = match std::env::var("YOUTUBE_API_KEY") {
        Ok(k) if !k.trim().is_empty() => Some(k),
        _ => None,
    };

    if let Ok(v) = std::env::var("TUBEFORGE_QUOTA_WARN_AT") {
        cfg.quota_warn_at = v
            .parse()
            .map_err(|_| TubeforgeError::Config(format!("TUBEFORGE_QUOTA_WARN_AT not a number: {v}")))?;
    }

    Ok(cfg)
}

impl Config {
    /// tantivy index location: `<data>/index/` (LLD §3.2; rebuildable, never
    /// part of backups).
    pub fn index_dir(&self) -> PathBuf {
        self.data_dir.join("index")
    }
}

fn load_env(cli_config: Option<&Path>) -> Result<(), TubeforgeError> {
    if let Some(path) = cli_config {
        dotenvy::from_path(path)
            .map_err(|e| TubeforgeError::Config(format!("cannot load env file {}: {e}", path.display())))?;
        return Ok(());
    }
    // CWD `.env` first, then the data-root `.env`.
    if Path::new(".env").exists() {
        dotenvy::from_filename(".env")
            .map_err(|e| TubeforgeError::Config(format!("cannot load .env: {e}")))?;
        return Ok(());
    }
    let home = home_dir().unwrap_or_default();
    let data_env = home.join(".tubeforge").join(".env");
    if data_env.exists() {
        dotenvy::from_path(&data_env)
            .map_err(|e| TubeforgeError::Config(format!("cannot load {}: {e}", data_env.display())))?;
    }
    Ok(())
}

/// Expand a leading `~` to the user's home directory.
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}
