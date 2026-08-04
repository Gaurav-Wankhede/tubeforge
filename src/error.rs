//! Error taxonomy per LLD §4.4, exit-code mapping per LLD §4.3.
//!
//! | Code | Meaning |
//! |------|---------|
//! | 0    | Success |
//! | 1    | Runtime/storage error |
//! | 2    | Usage error (clap) |
//! | 3    | Fetch/network error (retries exhausted) |
//! | 4    | Quota exhausted |
//! | 5    | Integrity failure (`integrity_check` failed) |

use std::fmt;

/// Data source for fetch/parse errors (LLD §4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Rss,
    OEmbed,
    Api,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::Rss => write!(f, "rss"),
            Source::OEmbed => write!(f, "oembed"),
            Source::Api => write!(f, "youtube-api"),
        }
    }
}

/// API endpoint for quota errors (LLD §4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    VideosList,
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Endpoint::VideosList => write!(f, "videos.list"),
        }
    }
}

/// The single error type for the whole binary (LLD §4.4).
#[derive(Debug, thiserror::Error)]
pub enum TubeforgeError {
    #[error("config error: {0}")]
    Config(String),

    #[error("fetch failed ({src} {url}): {inner}")]
    Fetch {
        src: Source,
        url: String,
        inner: String,
    },

    #[error("parse failed ({src} item={item}): {inner}")]
    Parse {
        src: Source,
        item: String,
        inner: String,
    },

    #[error("quota exhausted on {endpoint}: {remaining} units remaining")]
    Quota { endpoint: Endpoint, remaining: u64 },

    #[error("storage error ({code}): {message}")]
    Storage { code: String, message: String },

    #[error("integrity failure: {detail}")]
    Integrity { detail: String },

    #[error("index error: {detail}")]
    Index { detail: String },

    #[error("{0}")]
    Usage(String),

    /// Phase-gated feature that exists in the CLI but is not implemented yet.
    /// Renders as code NOT_IMPLEMENTED, exit 1 (Phase 0 gate: ingest links).
    #[error("not implemented: {0}")]
    NotImplemented(String),
}

impl TubeforgeError {
    /// Stable machine code rendered in the JSON envelope `error.code`.
    pub fn code(&self) -> &'static str {
        match self {
            TubeforgeError::Config(_) => "CONFIG",
            TubeforgeError::Fetch { .. } => "FETCH",
            TubeforgeError::Parse { .. } => "PARSE",
            TubeforgeError::Quota { .. } => "QUOTA_EXHAUSTED",
            TubeforgeError::Storage { .. } => "STORAGE",
            TubeforgeError::Integrity { .. } => "INTEGRITY",
            TubeforgeError::Index { .. } => "INDEX",
            TubeforgeError::Usage(_) => "USAGE",
            TubeforgeError::NotImplemented(_) => "NOT_IMPLEMENTED",
        }
    }

    /// Optional `source` field for the envelope (fetch/parse errors).
    pub fn source(&self) -> Option<String> {
        match self {
            TubeforgeError::Fetch { src, .. } | TubeforgeError::Parse { src, .. } => {
                Some(src.to_string())
            }
            TubeforgeError::Quota { endpoint, .. } => Some(endpoint.to_string()),
            _ => None,
        }
    }

    /// Optional `item` field for the envelope (fetch/parse errors).
    pub fn item(&self) -> Option<String> {
        match self {
            TubeforgeError::Fetch { url, .. } => Some(url.clone()),
            TubeforgeError::Parse { item, .. } => Some(item.clone()),
            _ => None,
        }
    }
}

/// Centralized exit-code mapping (LLD §4.3).
impl From<&TubeforgeError> for i32 {
    fn from(err: &TubeforgeError) -> i32 {
        match err {
            TubeforgeError::Config(_) => 1,
            TubeforgeError::Fetch { .. } => 3,
            TubeforgeError::Parse { .. } => 3,
            TubeforgeError::Quota { .. } => 4,
            TubeforgeError::Storage { .. } => 1,
            TubeforgeError::Integrity { .. } => 5,
            TubeforgeError::Index { .. } => 1,
            TubeforgeError::Usage(_) => 2,
            TubeforgeError::NotImplemented(_) => 1,
        }
    }
}

impl From<TubeforgeError> for i32 {
    fn from(err: TubeforgeError) -> i32 {
        i32::from(&err)
    }
}

impl From<std::io::Error> for TubeforgeError {
    fn from(err: std::io::Error) -> Self {
        TubeforgeError::Storage {
            code: "IO".to_string(),
            message: err.to_string(),
        }
    }
}

impl From<serde_json::Error> for TubeforgeError {
    fn from(err: serde_json::Error) -> Self {
        TubeforgeError::Storage {
            code: "JSON".to_string(),
            message: err.to_string(),
        }
    }
}

/// Convenience: wrap an engine error string into `Storage`.
pub fn storage_err(code: impl Into<String>, err: impl fmt::Display) -> TubeforgeError {
    TubeforgeError::Storage {
        code: code.into(),
        message: err.to_string(),
    }
}
