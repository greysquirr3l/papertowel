use serde_json::{Value, json};

use crate::protocol::RESULT_TYPE_COMPLETE;

/// Build a successful MCP tool-call result containing a single text block.
///
/// The result includes `resultType: "complete"` as required by the
/// `2026-07-28` spec, and `_meta` annotations for protocol introspection.
pub(super) fn tool_text(text: impl Into<String>) -> Value {
    json!({
        "resultType": RESULT_TYPE_COMPLETE,
        "content": [{ "type": "text", "text": text.into() }],
        "_meta": {
            "dev.greysquirr3l.papertowel/content-kind": "text"
        }
    })
}

/// Build a successful MCP tool-call result that signals a tool-level error.
pub(super) fn tool_error(message: impl Into<String>) -> Value {
    json!({
        "resultType": RESULT_TYPE_COMPLETE,
        "content": [{ "type": "text", "text": message.into() }],
        "isError": true,
        "_meta": {
            "dev.greysquirr3l.papertowel/content-kind": "error"
        }
    })
}

/// Build a successful MCP tool-call result containing both human-readable text
/// and structured JSON content.
///
/// `2026-07-28` recommends emitting the serialized JSON in a text block too
/// for backward compatibility with clients that don't yet read
/// `structuredContent`.
pub(super) fn tool_structured(text: impl Into<String>, structured: &Value) -> Value {
    json!({
        "resultType": RESULT_TYPE_COMPLETE,
        "content": [{ "type": "text", "text": text.into() }],
        "structuredContent": structured,
        "_meta": {
            "dev.greysquirr3l.papertowel/content-kind": "structured"
        }
    })
}