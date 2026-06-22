use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::resources::resource_templates_list;
use crate::tools::tools_list_page;

/// An incoming JSON-RPC 2.0 request or notification.
#[derive(Debug, Deserialize)]
pub struct IncomingMessage {
    pub jsonrpc: String,
    /// Absent for notifications. Per spec, request IDs MUST NOT be `null`.
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
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

// JSON-RPC 2.0 standard error codes.
pub const ERR_PARSE: i32 = -32700;
pub const ERR_INVALID_REQ: i32 = -32600;
pub const ERR_METHOD_NOT_FOUND: i32 = -32601;
/// Resource not found. In `2026-07-28` the code is `-32602` (Invalid Params);
/// in `2025-11-25` and earlier it was `-32002`. We expose a single constant
/// used by the dispatcher, which maps it to the negotiated version.
pub const ERR_RESOURCE_NOT_FOUND_INTERNAL: i32 = -32002;
pub const ERR_INVALID_PARAMS: i32 = -32602;
pub const ERR_INTERNAL: i32 = -32603;

/// MCP protocol versions supported by this server.
///
/// `2026-07-28` is the latest draft; the spec requires servers to remain
/// backward-compatible with at least the previous revision (`2025-11-25`)
/// during the deprecation window.
pub const PROTOCOL_VERSION_LATEST: &str = "2026-07-28";
pub const PROTOCOL_VERSION_PREVIOUS: &str = "2025-11-25";
pub const SERVER_NAME: &str = "papertowel";
pub const SERVER_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("PAPERTOWEL_GIT_SHA"),
    ")"
);

/// JSON Schema 2020-12 dialect URL used as `$schema` in tool input/output schemas.
pub const JSON_SCHEMA_DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";

/// Default page size for paginated `* /list` results.
pub const DEFAULT_PAGE_SIZE: usize = 25;

/// Default TTL for `CacheableResult.ttlMs` (5 seconds; lists are mostly static).
pub const DEFAULT_RESULT_TTL_MS: u32 = 5_000;

/// Required keys on the per-request `_meta` object under `2026-07-28`.
pub const META_KEY_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
pub const META_KEY_CLIENT_INFO: &str = "io.modelcontextprotocol/clientInfo";
pub const META_KEY_CLIENT_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";

/// `resultType` value emitted on every successful result.
pub const RESULT_TYPE_COMPLETE: &str = "complete";

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
        Self::err_with_data(id, code, message, None)
    }

    pub fn err_with_data(
        id: Value,
        code: i32,
        message: impl Into<String>,
        data: Option<Value>,
    ) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
                data,
            }),
        }
    }
}

/// Mutable per-connection state tracked across requests.
#[derive(Debug, Default)]
pub struct ServerState {
    inner: Mutex<ServerStateInner>,
}

#[derive(Debug, Default, Clone)]
struct ServerStateInner {
    /// Version negotiated during `initialize` or first per-request `_meta`.
    negotiated_version: Option<String>,
    /// Client information captured from `initialize` (or first per-request `_meta`).
    client_info: Option<Value>,
    /// Last-seen client capabilities.
    client_capabilities: Option<Value>,
    /// Current global log level (legacy `logging/setLevel` track).
    log_level: Option<String>,
}

impl ServerState {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(ServerStateInner {
                negotiated_version: None,
                client_info: None,
                client_capabilities: None,
                log_level: None,
            }),
        }
    }

    /// Negotiate and store the protocol version for this session.
    pub fn set_negotiated_version(&self, version: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.negotiated_version = Some(version.to_owned());
        }
    }

    /// Return the currently negotiated protocol version, if any.
    #[must_use]
    pub fn negotiated_version(&self) -> Option<String> {
        self.inner.lock().ok().and_then(|i| i.negotiated_version.clone())
    }

    /// Record client information from `initialize` or per-request `_meta`.
    pub fn set_client_info(&self, info: Value) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.client_info = Some(info);
        }
    }

    /// Update the current log level (legacy `logging/setLevel`).
    pub fn set_log_level(&self, level: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.log_level = Some(level.to_owned());
        }
    }

    /// Return the current log level, if any.
    #[must_use]
    pub fn log_level(&self) -> Option<String> {
        self.inner.lock().ok().and_then(|i| i.log_level.clone())
    }

    /// Record client capabilities.
    pub fn set_client_capabilities(&self, caps: Value) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.client_capabilities = Some(caps);
        }
    }

    /// Extract per-request `_meta` keys and update state. Returns the
    /// effective protocol version (negotiated or per-request override).
    pub fn observe_request_meta(&self, params: Option<&Value>) {
        let Some(meta) = params.and_then(|p| p.get("_meta")) else {
            return;
        };
        if let Some(version) = meta.get(META_KEY_PROTOCOL_VERSION).and_then(Value::as_str) {
            self.set_negotiated_version(version);
        }
        if let Some(info) = meta.get(META_KEY_CLIENT_INFO) {
            self.set_client_info(info.clone());
        }
        if let Some(caps) = meta.get(META_KEY_CLIENT_CAPABILITIES) {
            self.set_client_capabilities(caps.clone());
        }
    }
}

/// Translate an internal resource-not-found error code into the appropriate
/// code for the negotiated protocol version.
#[must_use]
pub fn resource_not_found_code(negotiated_version: Option<&str>) -> i32 {
    if negotiated_version == Some(PROTOCOL_VERSION_PREVIOUS) {
        ERR_RESOURCE_NOT_FOUND_INTERNAL
    } else {
        ERR_INVALID_PARAMS
    }
}

/// Return the list of MCP protocol versions this server supports.
#[must_use]
pub fn supported_versions() -> &'static [&'static str] {
    &[
        PROTOCOL_VERSION_LATEST,
        PROTOCOL_VERSION_PREVIOUS,
        "2025-06-18",
        "2025-03-26",
        "2024-11-05",
    ]
}

/// Negotiate a protocol version from the client's request.
#[must_use]
pub fn negotiate_protocol_version(requested: &str) -> &'static str {
    if supported_versions().contains(&requested) {
        // SAFETY: `requested` came from `supported_versions()` so the lookup
        // returns `Some`; map it back to the requested string.
        supported_versions()
            .iter()
            .copied()
            .find(|v| *v == requested)
            .unwrap_or(PROTOCOL_VERSION_LATEST)
    } else {
        PROTOCOL_VERSION_LATEST
    }
}

/// Build the standard `serverInfo` block.
fn server_info() -> Value {
    json!({
        "name": SERVER_NAME,
        "title": "papertowel MCP Server",
        "version": SERVER_VERSION,
        "description": "Scan, scrub, grade, and run cleanup workflows for AI-generated fingerprint patterns.",
        "websiteUrl": "https://github.com/greysquirr3l/papertowel"
    })
}

/// Build the capabilities block (advertised during `initialize` and `server/discover`).
fn capabilities_for(_negotiated_version: &str) -> Value {
    json!({
        "tools": { "listChanged": false },
        "prompts": { "listChanged": false },
        "resources": {
            "subscribe": false,
            "listChanged": false
        },
        "logging": {},
        "completions": {},
        "extensions": {
            // Tasks extension is advertised but not yet implemented; this
            // satisfies the `extensions` field contract from `2026-07-28`.
            "io.modelcontextprotocol/tasks": {}
        }
    })
}

/// Instructions emitted to clients during discovery.
fn instructions() -> &'static str {
    "Use papertowel_scan to detect AI-generated code fingerprints, papertowel_scrub for dry-run cleanup suggestions, papertowel_grade for overall AI fingerprint grading, papertowel_cleanup_assess to generate persisted cleanup reports, papertowel_cleanup_status to inspect deferred cleanup state, and papertowel_cleanup_apply to run policy-gated cleanup selection plus validation commands. papertowel also exposes workflow prompts (prompts/list), persisted cleanup reports as resources (resources/list), and argument completion (completion/complete). The legacy logging/setLevel method is deprecated; prefer per-request log level via _meta.io.modelcontextprotocol/logLevel."
}

/// Handle the `initialize` request.
///
/// Kept for backward compatibility with `2025-11-25` and earlier clients.
/// `2026-07-28` clients may use `server/discover` instead and skip this round
/// trip entirely.
pub fn handle_initialize(params: Option<&Value>, state: &ServerState) -> Value {
    let requested = params
        .and_then(|p| p.get("protocolVersion"))
        .and_then(Value::as_str)
        .unwrap_or(PROTOCOL_VERSION_LATEST);
    let negotiated = negotiate_protocol_version(requested);

    state.set_negotiated_version(negotiated);
    if let Some(params) = params {
        if let Some(info) = params.get("clientInfo") {
            state.set_client_info(info.clone());
        }
        if let Some(caps) = params.get("capabilities") {
            state.set_client_capabilities(caps.clone());
        }
    }

    json!({
        "protocolVersion": negotiated,
        "capabilities": capabilities_for(negotiated),
        "serverInfo": server_info(),
        "instructions": instructions(),
        "_meta": {
            "dev.greysquirr3l.papertowel/upgrade-mode": if negotiated == PROTOCOL_VERSION_LATEST {
                "draft-2026-07-28"
            } else {
                "stable-2025-11-25"
            }
        }
    })
}

/// Handle the `server/discover` request (introduced in `2026-07-28`).
///
/// Clients use this to discover supported protocol versions and capabilities
/// up-front without performing the full `initialize` handshake. Per the spec,
/// clients MAY call this before any other request.
pub fn handle_server_discover(_params: Option<&Value>) -> Value {
    let supported = supported_versions()
        .iter()
        .map(|v| Value::String((*v).to_owned()))
        .collect::<Vec<_>>();
    json!({
        "resultType": RESULT_TYPE_COMPLETE,
        "supportedVersions": supported,
        "capabilities": capabilities_for(PROTOCOL_VERSION_LATEST),
        "serverInfo": server_info(),
        "instructions": instructions(),
        "_meta": {
            "dev.greysquirr3l.papertowel/upgrade-mode": "draft-2026-07-28"
        }
    })
}

/// Handle `tools/list` with cursor-based pagination.
pub fn handle_tools_list(params: Option<&Value>) -> Value {
    let cursor = params
        .and_then(|p| p.get("cursor"))
        .and_then(Value::as_str);
    tools_list_page(cursor)
}

/// Handle `prompts/list` with cursor-based pagination.
pub fn handle_prompts_list(params: Option<&Value>) -> Value {
    let cursor = params
        .and_then(|p| p.get("cursor"))
        .and_then(Value::as_str);
    crate::prompts::prompts_list_page(cursor)
}

/// Handle `resources/list` with cursor-based pagination.
pub fn handle_resources_list(params: Option<&Value>) -> Value {
    let cursor = params
        .and_then(|p| p.get("cursor"))
        .and_then(Value::as_str);
    crate::resources::resources_list_page(cursor)
}

/// Handle `resources/templates/list`.
pub fn handle_resource_templates_list(_params: Option<&Value>) -> Value {
    json!({
        "resultType": RESULT_TYPE_COMPLETE,
        "ttlMs": DEFAULT_RESULT_TTL_MS,
        "cacheScope": "public",
        "resourceTemplates": resource_templates_list(),
    })
}

/// Handle a `prompts/get` request by name.
pub fn handle_prompts_get(params: Option<&Value>) -> Result<Value, anyhow::Error> {
    let params = params.ok_or_else(|| anyhow::anyhow!("invalid params: missing params object"))?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("invalid params: missing prompt name"))?;
    let arguments = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
    crate::prompts::get_prompt(name, &arguments)
        .ok_or_else(|| anyhow::anyhow!("invalid params: unknown prompt '{name}'"))
}

/// Handle a `resources/read` request by URI.
pub fn handle_resources_read(params: Option<&Value>) -> Result<Value, anyhow::Error> {
    let params = params.ok_or_else(|| anyhow::anyhow!("invalid params: missing params object"))?;
    let uri = params
        .get("uri")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("invalid params: missing resource URI"))?;
    crate::resources::read_resource(uri)
        .ok_or_else(|| anyhow::anyhow!("resource not found: {uri}"))
}

/// Handle a `completion/complete` request for prompt or resource argument suggestions.
pub fn handle_completion_complete(params: Option<&Value>) -> Result<Value, anyhow::Error> {
    let params = params.ok_or_else(|| anyhow::anyhow!("invalid params: missing params object"))?;
    let ref_value = params
        .get("ref")
        .ok_or_else(|| anyhow::anyhow!("invalid params: missing 'ref' object"))?;
    let argument = params
        .get("argument")
        .ok_or_else(|| anyhow::anyhow!("invalid params: missing 'argument' object"))?;
    crate::completion::complete(ref_value, argument)
}

/// Handle a `logging/setLevel` request.
///
/// Deprecated as of protocol version `2026-07-28` (SEP-2577). Kept for
/// backward compatibility with `2025-11-25` and earlier clients.
pub fn handle_logging_set_level(
    params: Option<&Value>,
    state: &ServerState,
) -> Result<Value, anyhow::Error> {
    let params = params.ok_or_else(|| anyhow::anyhow!("invalid params: missing params object"))?;
    let level = params
        .get("level")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("invalid params: missing 'level'"))?;
    crate::logging::set_log_level(level)
        .ok_or_else(|| anyhow::anyhow!("invalid params: invalid log level '{level}'"))?;
    state.set_log_level(level);
    Ok(json!({ "resultType": RESULT_TYPE_COMPLETE }))
}

/// Translate an `anyhow::Error` into the appropriate JSON-RPC error code.
pub fn method_result_code(error: &anyhow::Error) -> i32 {
    let message = error.to_string();
    if message.starts_with("method not found") {
        ERR_METHOD_NOT_FOUND
    } else if message.starts_with("invalid params") {
        ERR_INVALID_PARAMS
    } else {
        ERR_INTERNAL
    }
}