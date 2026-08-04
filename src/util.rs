//! Small shared helpers (timestamps, path scanning) used across modules.

use chrono::{SecondsFormat, Utc};

/// Lowercase alphanumeric tokens — the shared tokenizer for overlap/Jaccard
/// math in scoring and analytics (deterministic; no stopword list).
pub fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(|t| t.to_lowercase())
        .filter(|t| !t.is_empty())
        .collect()
}

/// Current UTC time as RFC3339 with seconds precision (stored as TEXT,
/// lexicographically comparable — LLD §3.1).
pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Current UTC time as `YYYYMMDDTHHMMSSZ` (compact ingest batch id).
pub fn batch_id() -> String {
    Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

/// `true` when `prog` exists somewhere on `PATH` (used by `tubeforge mcp`).
pub fn on_path(prog: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                let full = dir.join(prog);
                full.is_file() && is_executable(&full)
            })
        })
        .unwrap_or(false)
}

#[cfg(unix)]
fn is_executable(p: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(p: &std::path::Path) -> bool {
    p.is_file()
}
