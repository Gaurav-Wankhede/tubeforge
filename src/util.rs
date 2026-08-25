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

/// Simple alphanumeric nanoid generation.
pub fn nanoid(len: usize) -> String {
    use std::time::SystemTime;
    let chars = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(42);
    let mut state = nanos as u64;
    let mut out = String::with_capacity(len);
    for _ in 0..len {
        // xorshift64
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let idx = (state as usize) % chars.len();
        out.push(chars[idx] as char);
    }
    out
}
