use serde_json::{Value, json};

/// Build a successful MCP tool-call result containing a single text block.
pub(super) fn tool_text(text: impl Into<String>) -> Value {
    json!({
        "content": [{ "type": "text", "text": text.into() }]
    })
}

/// Build a successful MCP tool-call result that signals a tool-level error.
pub(super) fn tool_error(message: impl Into<String>) -> Value {
    json!({
        "content": [{ "type": "text", "text": message.into() }],
        "isError": true
    })
}
