//! Property-based tests for the RPC protocol serialization.
//!
//! Verifies that RpcRequest/RpcResponse messages survive JSON roundtrip
//! and that the protocol is well-formed for any valid input.

use proptest::prelude::*;
use serde::{Deserialize, Serialize};

/// Mirror of the server's RpcRequest (kept in sync manually for testing).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct RpcRequest {
    id: serde_json::Value,
    method: String,
    #[serde(default)]
    params: serde_json::Value,
}

/// Mirror of the server's RpcResponse variants.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RpcResponse {
    Progress {
        id: serde_json::Value,
        progress: f32,
        message: String,
    },
    Result {
        id: serde_json::Value,
        data: serde_json::Value,
    },
    Error {
        id: serde_json::Value,
        error: RpcError,
    },
    Notification {
        event: String,
        data: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct RpcError {
    code: i32,
    message: String,
}

/// Strategy: valid JSON value (for id/params/data).
fn json_value() -> impl Strategy<Value = serde_json::Value> {
    prop::strategy::Union::new_weighted(vec![
        (1, Just(serde_json::Value::Null).boxed()),
        (2, prop::bool::ANY.prop_map(serde_json::Value::from).boxed()),
        (
            3,
            prop::num::i64::ANY
                .prop_map(serde_json::Value::from)
                .boxed(),
        ),
        (
            3,
            "[a-z0-9]{1,20}".prop_map(serde_json::Value::from).boxed(),
        ),
    ])
}

/// Strategy: RPC method name.
fn method_name() -> impl Strategy<Value = String> {
    "[a-z]{3,10}\\.[a-z]{3,10}"
}

proptest! {
    /// PROPERTY: RpcRequest survives JSON roundtrip (serialize → deserialize).
    #[test]
    fn request_roundtrip(
        id in json_value(),
        method in method_name(),
        params in json_value(),
    ) {
        let req = RpcRequest { id, method, params };
        let json = serde_json::to_string(&req).expect("serialize");
        let decoded: RpcRequest = serde_json::from_str(&json).expect("deserialize");
        prop_assert_eq!(req, decoded, "request roundtrip failed");
    }

    /// PROPERTY: RpcResponse::Progress survives JSON roundtrip.
    #[test]
    fn progress_roundtrip(
        id in json_value(),
        progress in prop::num::f64::POSITIVE.prop_filter("0-1", |&p| p <= 1.0),
        message in "[a-zA-Z ]{1,50}",
    ) {
        let res = RpcResponse::Progress { id, progress: progress as f32, message };
        let json = serde_json::to_string(&res).expect("serialize");
        let decoded: RpcResponse = serde_json::from_str(&json).expect("deserialize");
        prop_assert_eq!(res, decoded, "progress roundtrip failed");
    }

    /// PROPERTY: RpcResponse::Result survives JSON roundtrip.
    #[test]
    fn result_roundtrip(
        id in json_value(),
        data in json_value(),
    ) {
        let res = RpcResponse::Result { id, data };
        let json = serde_json::to_string(&res).expect("serialize");
        let decoded: RpcResponse = serde_json::from_str(&json).expect("deserialize");
        prop_assert_eq!(res, decoded, "result roundtrip failed");
    }

    /// PROPERTY: RpcResponse::Error survives JSON roundtrip.
    #[test]
    fn error_roundtrip(
        id in json_value(),
        code in prop::num::i32::ANY,
        message in "[a-zA-Z ]{1,50}",
    ) {
        let res = RpcResponse::Error {
            id,
            error: RpcError { code, message },
        };
        let json = serde_json::to_string(&res).expect("serialize");
        let decoded: RpcResponse = serde_json::from_str(&json).expect("deserialize");
        prop_assert_eq!(res, decoded, "error roundtrip failed");
    }

    /// PROPERTY: RpcResponse::Notification survives JSON roundtrip.
    #[test]
    fn notification_roundtrip(
        event in "[a-z_]{3,20}",
        data in json_value(),
    ) {
        let res = RpcResponse::Notification { event, data };
        let json = serde_json::to_string(&res).expect("serialize");
        let decoded: RpcResponse = serde_json::from_str(&json).expect("deserialize");
        prop_assert_eq!(res, decoded, "notification roundtrip failed");
    }

    /// PROPERTY: Serialized response has a "type" field (tagged enum).
    #[test]
    fn response_has_type_field(
        id in json_value(),
        data in json_value(),
    ) {
        let res = RpcResponse::Result { id, data };
        let json = serde_json::to_string(&res).expect("serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
        prop_assert!(
            parsed.get("type").is_some(),
            "serialized response missing 'type' field: {}", json
        );
        prop_assert_eq!(
            &parsed["type"], "result",
            "type field should be 'result'"
        );
    }

    /// PROPERTY: Unknown method names are still valid JSON (forward compat).
    #[test]
    fn unknown_method_still_valid_json(
        method in "[a-z]{1,30}",
        id in json_value(),
    ) {
        let req = RpcRequest { id, method, params: serde_json::Value::Null };
        let json = serde_json::to_string(&req).expect("serialize");
        let decoded: RpcRequest = serde_json::from_str(&json).expect("deserialize");
        prop_assert_eq!(decoded.method, req.method, "method should roundtrip");
    }
}
