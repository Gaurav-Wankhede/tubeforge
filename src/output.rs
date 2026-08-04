//! JSON envelope (LLD §4.2) and human rendering.

use serde::Serialize;
use serde_json::{json, Value};

use crate::error::TubeforgeError;

/// Quota payload inside `meta` (LLD §4.2).
#[derive(Debug, Clone, Serialize)]
pub struct QuotaInfo {
    pub videos_list_used: u64,
    pub daily_limit: u64,
}

/// `meta` object of the envelope.
#[derive(Debug, Clone, Serialize)]
pub struct Meta {
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota: Option<QuotaInfo>,
}

/// `error` object of the envelope (LLD §4.2).
#[derive(Debug, Clone, Serialize)]
pub struct ErrorDetail {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
}

/// Stable envelope: `{ ok, data, meta, error }`.
#[derive(Debug, Clone, Serialize)]
pub struct Envelope {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorDetail>,
}

impl Envelope {
    pub fn ok(data: Value, meta: Option<Meta>) -> Self {
        Envelope {
            ok: true,
            data: Some(data),
            meta,
            error: None,
        }
    }

    pub fn error(err: &TubeforgeError) -> Self {
        Envelope {
            ok: false,
            data: None,
            meta: None,
            error: Some(ErrorDetail {
                code: err.code().to_string(),
                message: err.to_string(),
                source: err.source(),
                item: err.item(),
            }),
        }
    }

    /// Serialize to a compact single-line JSON object.
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).expect("envelope is always serializable")
    }
}

/// Render an envelope to stdout in the requested mode.
///
/// JSON mode: always the full envelope. Human mode: data on stdout (errors
/// go to stderr from the caller per LLD §4.4).
pub fn render(envelope: &Envelope, json: bool) {
    if json {
        println!("{}", serde_json::to_string(envelope).expect("serialize"));
    } else if envelope.error.is_none() {
        if let Some(data) = &envelope.data {
            println!("{}", data);
        }
    }
}

/// Convenience: build `meta` with a duration and optional quota.
pub fn meta(duration_ms: u64, quota: Option<QuotaInfo>) -> Meta {
    Meta { duration_ms, quota }
}

/// Quota info as a JSON value for `data`.
pub fn quota_json(used: u64, daily_limit: u64, date: Option<&str>) -> Value {
    json!({
        "videos_list": {
            "used": used,
            "daily_limit": daily_limit,
            "date": date,
        }
    })
}
