use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// An incoming JSON-RPC 2.0 request or notification.
#[derive(Debug, Deserialize)]
pub struct IncomingMessage {
    pub jsonrpc: String,
    /// Absent for notifications.
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

/// An outgoing JSON-RPC 2.0 response.
#[derive(Debug, Serialize)]
pub struct Response {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

// JSON-RPC error codes
pub const ERR_PARSE: i32 = -32700;
pub const ERR_INVALID_REQ: i32 = -32600;
pub const ERR_METHOD_NOT_FOUND: i32 = -32601;
pub const ERR_INVALID_PARAMS: i32 = -32602;
pub const ERR_INTERNAL: i32 = -32603;

// MCP protocol constants
pub const PROTOCOL_VERSION: &str = "2025-11-25";
pub const SERVER_NAME: &str = "papertowel";
pub const SERVER_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("PAPERTOWEL_GIT_SHA"),
    ")"
);

impl Response {
    #[expect(
        clippy::missing_const_for_fn,
        reason = "serde_json::Value is not const-constructible"
    )]
    pub fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

pub fn handle_initialize(params: Option<&Value>) -> Value {
    // Negotiate protocol version: echo the client's version if we support it,
    // otherwise respond with the latest version we support.
    const SUPPORTED_VERSIONS: &[&str] = &["2025-11-25", "2025-03-26", "2024-11-05"];
    let requested = params
        .and_then(|p| p.get("protocolVersion"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let negotiated = if SUPPORTED_VERSIONS.contains(&requested) {
        requested
    } else {
        PROTOCOL_VERSION
    };

    json!({
        "protocolVersion": negotiated,
        "capabilities": {
            "tools": {
                "listChanged": false
            }
        },
        "serverInfo": {
            "name": SERVER_NAME,
            "title": "papertowel MCP Server",
            "version": SERVER_VERSION,
            "description": "Scan, scrub, grade, and run cleanup workflows for AI-generated fingerprint patterns."
        },
        "instructions": "Use papertowel_scan to detect AI-generated code fingerprints, papertowel_scrub for dry-run cleanup suggestions, papertowel_grade for overall AI fingerprint grading, papertowel_cleanup_assess to generate persisted cleanup reports, papertowel_cleanup_status to inspect deferred cleanup state, and papertowel_cleanup_apply to run policy-gated cleanup selection plus validation commands."
    })
}

pub fn method_result_code(error: &anyhow::Error) -> i32 {
    if error.to_string().starts_with("method not found") {
        ERR_METHOD_NOT_FOUND
    } else if error.to_string().starts_with("invalid params") {
        ERR_INVALID_PARAMS
    } else {
        ERR_INTERNAL
    }
}
