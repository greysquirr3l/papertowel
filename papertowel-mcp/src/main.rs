#![expect(
    clippy::multiple_crate_versions,
    reason = "transitive dependency graph currently includes duplicate versions"
)]

//! `papertowel-mcp` — MCP server that exposes papertowel scan and scrub
//! capabilities as tools, prompts, and resources consumable by LLM clients
//! (e.g. Claude Desktop, Cursor, Continue.dev).
//!
//! # Protocol
//!
//! Implements the MCP stdio transport against the latest published revision
//! (`2026-07-28` draft) and remains backward-compatible with `2025-11-25`.
//! Each message is a single UTF-8 JSON object followed by a newline (`\n`).
//! Embedded newlines are not permitted inside a message.

use std::io::{self, Write};

use anyhow::Result;
use serde_json::Value;
use tracing::{debug, error, info, instrument, warn};

mod completion;
mod logging;
mod path_guard;
mod prompts;
mod protocol;
mod resources;
mod tools;
mod transport;

use protocol::{
    ERR_INVALID_REQ, ERR_PARSE, IncomingMessage, Response, ServerState,
    handle_completion_complete, handle_initialize, handle_logging_set_level, handle_prompts_get,
    handle_prompts_list, handle_resource_templates_list, handle_resources_list, handle_resources_read,
    handle_server_discover, handle_tools_list, method_result_code,
};
use tools::{handle_tools_call};
use transport::{read_message, write_response};

fn write_response_observed(
    writer: &mut impl Write,
    resp: &Response,
    phase: &str,
    method: Option<&str>,
) {
    if let Err(e) = write_response(resp, writer) {
        error!(error = %e, phase, method, "failed to write response");
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!("papertowel-mcp starting (protocol {} draft, 2025-11-25 compatible)", protocol::PROTOCOL_VERSION_LATEST);

    let state = ServerState::new();
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = io::BufReader::new(stdin.lock());
    let mut writer = io::BufWriter::new(stdout.lock());

    loop {
        match read_message(&mut reader) {
            Ok(None) => {
                info!("stdin closed; exiting");
                break;
            }
            Ok(Some(raw)) => {
                debug!(raw = %raw, "received message");
                handle_raw(&raw, &state, &mut writer);
            }
            Err(e) => {
                error!(error = %e, "failed to read message");
                let resp = Response::err(Value::Null, ERR_PARSE, format!("read error: {e}"));
                write_response_observed(&mut writer, &resp, "read_error", None);
            }
        }
    }
}

/// Parse raw JSON and dispatch to the appropriate handler.
#[instrument(skip_all, fields(raw))]
fn handle_raw(raw: &str, state: &ServerState, writer: &mut impl Write) {
    let msg: IncomingMessage = match serde_json::from_str(raw) {
        Ok(m) => m,
        Err(e) => {
            let resp = Response::err(Value::Null, ERR_PARSE, format!("invalid JSON: {e}"));
            write_response_observed(writer, &resp, "parse_error", None);
            return;
        }
    };

    if msg.jsonrpc != "2.0" {
        if let Some(id) = msg.id {
            let resp = Response::err(id, ERR_INVALID_REQ, "jsonrpc must be \"2.0\"");
            write_response_observed(writer, &resp, "invalid_jsonrpc", Some(msg.method.as_str()));
        }
        return;
    }

    // Extract per-request `_meta` so we can observe protocol version + identity.
    state.observe_request_meta(msg.params.as_ref());

    // Notifications (no id) are processed but never get a response.
    let is_notification = msg.id.is_none();
    let method_name = msg.method.clone();

    let result: Result<Value> = match msg.method.as_str() {
        // Lifecycle
        "initialize" => Ok(handle_initialize(msg.params.as_ref(), state)),
        "server/discover" => Ok(handle_server_discover(msg.params.as_ref())),

        // Tools
        "tools/list" => Ok(handle_tools_list(msg.params.as_ref())),
        "tools/call" => handle_tools_call(msg.params.as_ref()),

        // Prompts
        "prompts/list" => Ok(handle_prompts_list(msg.params.as_ref())),
        "prompts/get" => handle_prompts_get(msg.params.as_ref()),

        // Resources
        "resources/list" => Ok(handle_resources_list(msg.params.as_ref())),
        "resources/templates/list" => Ok(handle_resource_templates_list(msg.params.as_ref())),
        "resources/read" => handle_resources_read(msg.params.as_ref()),

        // Completion + logging
        "completion/complete" => handle_completion_complete(msg.params.as_ref()),
        "logging/setLevel" => handle_logging_set_level(msg.params.as_ref(), state),

        // Ping (kept for back-compat; deprecated under 2026-07-28 but spec still mandates support)
        "ping" => Ok(serde_json::json!({ "resultType": protocol::RESULT_TYPE_COMPLETE })),

        // Notifications (no response)
        "notifications/initialized" => {
            debug!("received initialized notification");
            return;
        }
        "notifications/cancelled" => {
            debug!("received cancelled notification");
            return;
        }

        method => {
            warn!(method, "unknown method");
            if is_notification {
                return;
            }
            Err(anyhow::anyhow!("method not found: {method}"))
        }
    };

    if is_notification {
        return;
    }

    let id = msg.id.unwrap_or(Value::Null);
    let resp = match result {
        Ok(r) => Response::ok(id, r),
        Err(e) => {
            let code = method_result_code(&e);
            // Resource-not-found uses the version-aware code under 2026-07-28.
            let code = if e.to_string().starts_with("resource not found") {
                protocol::resource_not_found_code(state.negotiated_version().as_deref())
            } else {
                code
            };
            Response::err(id, code, e.to_string())
        }
    };

    write_response_observed(writer, &resp, "method_result", Some(method_name.as_str()));
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use tempfile::TempDir;

    use crate::completion::complete;
    use crate::path_guard::validate_mcp_path;
    use crate::prompts::prompts_list_page;
    use crate::protocol::{
        ERR_INVALID_PARAMS, ERR_RESOURCE_NOT_FOUND_INTERNAL, PROTOCOL_VERSION_LATEST,
        PROTOCOL_VERSION_PREVIOUS, ServerState, handle_initialize, handle_logging_set_level,
        handle_prompts_get, handle_resource_templates_list, handle_resources_read,
        handle_server_discover,
    };
    use crate::resources::resources_list_page;
    use crate::tools::handle_tools_list;

    #[test]
    fn initialize_negotiates_latest_when_unsupported_version_is_requested() {
        let state = ServerState::new();
        let init =
            handle_initialize(Some(&json!({ "protocolVersion": "2099-01-01" })), &state);
        assert_eq!(
            init.get("protocolVersion"),
            Some(&json!(PROTOCOL_VERSION_LATEST))
        );
        assert_eq!(state.negotiated_version().as_deref(), Some(PROTOCOL_VERSION_LATEST));
    }

    #[test]
    fn initialize_negotiates_previous_when_previous_version_is_requested() {
        let state = ServerState::new();
        let init = handle_initialize(
            Some(&json!({ "protocolVersion": PROTOCOL_VERSION_PREVIOUS })),
            &state,
        );
        assert_eq!(
            init.get("protocolVersion"),
            Some(&json!(PROTOCOL_VERSION_PREVIOUS))
        );
        assert_eq!(
            state.negotiated_version().as_deref(),
            Some(PROTOCOL_VERSION_PREVIOUS)
        );
    }

    #[test]
    fn initialize_advertises_required_capabilities_and_extensions() {
        let state = ServerState::new();
        let init = handle_initialize(
            Some(&json!({ "protocolVersion": PROTOCOL_VERSION_LATEST })),
            &state,
        );

        let caps_present = init.get("capabilities").is_some();
        assert!(caps_present, "capabilities must be present");
        let caps = &init;
        assert!(caps.get("capabilities")
            .and_then(|c| c.get("tools"))
            .is_some());
        assert!(caps.get("capabilities")
            .and_then(|c| c.get("prompts"))
            .is_some());
        assert!(caps.get("capabilities")
            .and_then(|c| c.get("resources"))
            .is_some());
        assert!(caps.get("capabilities")
            .and_then(|c| c.get("completions"))
            .is_some());
        assert!(caps.get("capabilities")
            .and_then(|c| c.get("logging"))
            .is_some());
        assert!(caps.get("capabilities")
            .and_then(|c| c.get("extensions"))
            .and_then(|e| e.get("io.modelcontextprotocol/tasks"))
            .is_some());

        let server_info = &init;
        assert!(server_info.get("serverInfo").and_then(|i| i.get("websiteUrl")).is_some());
        assert!(server_info.get("serverInfo").and_then(|i| i.get("title")).is_some());
        assert!(server_info.get("serverInfo").and_then(|i| i.get("description")).is_some());
    }

    #[test]
    fn server_discover_advertises_supported_versions() {
        let result = handle_server_discover(Some(&json!({})));
        assert_eq!(result.get("resultType"), Some(&json!("complete")));

        let has_latest = result
            .get("supportedVersions")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|v| v.iter().any(|ver| ver == &json!(PROTOCOL_VERSION_LATEST)));
        assert!(has_latest, "latest version must be in supportedVersions");

        let has_previous = result
            .get("supportedVersions")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|v| v.iter().any(|ver| ver == &json!(PROTOCOL_VERSION_PREVIOUS)));
        assert!(has_previous, "previous version must be in supportedVersions");
    }

    #[test]
    fn resource_not_found_code_varies_by_negotiated_version() {
        use crate::protocol::resource_not_found_code;
        assert_eq!(
            resource_not_found_code(Some(PROTOCOL_VERSION_LATEST)),
            ERR_INVALID_PARAMS,
            "2026-07-28 must use -32602"
        );
        assert_eq!(
            resource_not_found_code(Some(PROTOCOL_VERSION_PREVIOUS)),
            ERR_RESOURCE_NOT_FOUND_INTERNAL,
            "2025-11-25 must use -32002"
        );
    }

    #[test]
    fn tools_list_is_sorted_and_carries_cacheable_result() {
        let list = handle_tools_list();
        assert_eq!(list.get("resultType"), Some(&json!("complete")));
        assert_eq!(list.get("cacheScope"), Some(&json!("public")));
        let has_ttl = list
            .get("ttlMs")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|t| t > 0);
        assert!(has_ttl);

        let names: Vec<String> = list
            .get("tools")
            .and_then(serde_json::Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|t| {
                        t.get("name")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(
            names, sorted,
            "tools must be returned alphabetically for deterministic caching"
        );
    }

    #[test]
    fn tools_all_carry_json_schema_dialect() {
        let list = handle_tools_list();
        let tools_present = list
            .get("tools")
            .and_then(serde_json::Value::as_array)
            .is_some();
        assert!(tools_present, "tools list must be present");
        if let Some(tools) = list.get("tools").and_then(serde_json::Value::as_array) {
            for tool in tools {
                let has_dialect = tool
                    .get("inputSchema")
                    .and_then(|s| s.get("$schema"))
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|d| d == crate::protocol::JSON_SCHEMA_DIALECT);
                let name = tool
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("?");
                assert!(has_dialect, "{name} must declare JSON Schema 2020-12 dialect");
            }
        }
    }

    #[test]
    fn cleanup_tools_have_output_schemas() {
        let list = handle_tools_list();
        let all_present = [
            "papertowel_cleanup_assess",
            "papertowel_cleanup_status",
            "papertowel_cleanup_apply",
        ]
        .iter()
        .all(|name| {
            list.get("tools")
                .and_then(serde_json::Value::as_array)
                .and_then(|arr| {
                    arr.iter()
                        .find(|t| t.get("name") == Some(&json!(name)))
                })
                .is_some_and(|t| {
                    t.get("outputSchema")
                        .and_then(|s| s.get("$schema"))
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|d| d == crate::protocol::JSON_SCHEMA_DIALECT)
                })
        });
        assert!(
            all_present,
            "all cleanup tools must declare an outputSchema with 2020-12 dialect"
        );
    }

    #[test]
    fn cleanup_apply_advertises_task_support() {
        let list = handle_tools_list();
        let tools = list
            .get("tools")
            .and_then(serde_json::Value::as_array);
        let apply = tools.and_then(|arr| {
            arr.iter()
                .find(|t| t.get("name") == Some(&json!("papertowel_cleanup_apply")))
        });
        let has_task_support = apply
            .and_then(|t| t.get("execution"))
            .and_then(|e| e.get("taskSupport"))
            .is_some_and(|v| v == &json!("optional"));
        assert!(has_task_support, "cleanup_apply should advertise taskSupport=optional");
    }

    #[test]
    fn prompts_list_is_sorted_and_cacheable() {
        let page = prompts_list_page(None);
        assert_eq!(page.get("resultType"), Some(&json!("complete")));
        assert_eq!(page.get("cacheScope"), Some(&json!("public")));
        let names: Vec<String> = page
            .get("prompts")
            .and_then(serde_json::Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| {
                        p.get("name")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned)
                    })
                    .collect()
            })
            .unwrap_or_default();
        assert!(!names.is_empty());
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn prompts_get_unknown_returns_invalid_params() {
        let result = handle_prompts_get(Some(&json!({ "name": "nope" })));
        let is_invalid_params = result
            .is_err_and(|e| e.to_string().contains("invalid params"));
        assert!(is_invalid_params);
    }

    #[test]
    fn resources_list_is_sorted_and_cacheable() {
        let page = resources_list_page(None);
        assert_eq!(page.get("resultType"), Some(&json!("complete")));
        assert_eq!(page.get("cacheScope"), Some(&json!("public")));
        let uris: Vec<String> = page
            .get("resources")
            .and_then(serde_json::Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|r| {
                        r.get("uri")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut sorted = uris.clone();
        sorted.sort();
        assert_eq!(uris, sorted);
    }

    #[test]
    fn resources_read_unknown_uri_returns_invalid_params() {
        let result = handle_resources_read(Some(&json!({ "uri": "papertowel://nope" })));
        assert!(result.is_err());
    }

    #[test]
    fn resource_templates_list_includes_cacheable_result() {
        let result = handle_resource_templates_list(None);
        assert_eq!(result.get("resultType"), Some(&json!("complete")));
        assert_eq!(result.get("cacheScope"), Some(&json!("public")));
        let has_ttl = result
            .get("ttlMs")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|t| t > 0);
        assert!(has_ttl);
    }

    #[test]
    fn completion_supports_prompt_and_resource_refs() {
        let prompt_ref = json!({ "type": "ref/prompt", "name": "ai-fingerprint-audit" });
        let arg = json!({ "name": "min_severity", "value": "h" });
        let result = complete(&prompt_ref, &arg);
        let prompt_has_high = result.is_ok_and(|v| {
            v.get("completion")
                .and_then(|c| c.get("values"))
                .and_then(serde_json::Value::as_array)
                .is_some_and(|arr| arr.iter().any(|v| v == &json!("high")))
        });
        assert!(prompt_has_high);

        let resource_ref =
            json!({ "type": "ref/resource", "uri": "papertowel://cleanup/{name}" });
        let arg = json!({ "name": "name", "value": "la" });
        let result = complete(&resource_ref, &arg);
        let resource_has_latest = result.is_ok_and(|v| {
            v.get("completion")
                .and_then(|c| c.get("values"))
                .and_then(serde_json::Value::as_array)
                .is_some_and(|arr| arr.iter().any(|v| v == &json!("latest")))
        });
        assert!(resource_has_latest);
    }

    #[test]
    fn logging_set_level_still_supported_for_backcompat() {
        let state = ServerState::new();
        let result = handle_logging_set_level(Some(&json!({ "level": "info" })), &state);
        assert!(result.is_ok());
        assert_eq!(state.log_level().as_deref(), Some("info"));

        let bad = handle_logging_set_level(Some(&json!({ "level": "verbose" })), &state);
        assert!(bad.is_err());
    }

    #[test]
    fn protocol_surface_for_initialize_and_tools_list_is_stable() {
        let state = ServerState::new();
        let init =
            handle_initialize(Some(&json!({ "protocolVersion": "2025-11-25" })), &state);

        assert_eq!(
            init.get("protocolVersion"),
            Some(&json!("2025-11-25")),
            "initialize should negotiate and return the negotiated protocol version"
        );
        assert_eq!(
            init.get("capabilities")
                .and_then(|cap| cap.get("tools"))
                .and_then(|tools| tools.get("listChanged")),
            Some(&json!(false)),
            "initialize should expose tools.listChanged capability"
        );
        assert_eq!(
            init.get("serverInfo")
                .and_then(|info| info.get("description")),
            Some(&json!(
                "Scan, scrub, grade, and run cleanup workflows for AI-generated fingerprint patterns."
            )),
            "initialize should include serverInfo.description"
        );

        let tools_list = handle_tools_list();
        let tools_present = tools_list
            .get("tools")
            .and_then(serde_json::Value::as_array)
            .is_some();
        assert!(tools_present, "tools/list should return a tools array");

        let expected_tools = [
            ("papertowel_scan", true, false, true, false),
            ("papertowel_scrub", true, false, true, false),
            ("papertowel_grade", true, false, true, false),
            ("papertowel_cleanup_assess", false, false, true, false),
            ("papertowel_cleanup_status", true, false, true, false),
            ("papertowel_cleanup_apply", false, true, false, true),
        ];
        let all_match = expected_tools.iter().all(|(expected_name, read_only, destructive, idempotent, open_world)| {
            let Some(tool) = tools_list
                .get("tools")
                .and_then(serde_json::Value::as_array)
                .and_then(|arr| {
                    arr.iter().find(|t| t.get("name") == Some(&json!(*expected_name)))
                })
            else {
                return false;
            };
            tool.get("annotations").and_then(|ann| ann.get("readOnlyHint")) == Some(&json!(*read_only))
                && tool.get("annotations").and_then(|ann| ann.get("destructiveHint")) == Some(&json!(*destructive))
                && tool.get("annotations").and_then(|ann| ann.get("idempotentHint")) == Some(&json!(*idempotent))
                && tool.get("annotations").and_then(|ann| ann.get("openWorldHint")) == Some(&json!(*open_world))
        });
        assert!(all_match, "all expected tools should match annotation contract");
    }

    #[test]
    fn valid_project_path_passes() {
        let Ok(dir) = TempDir::new() else {
            return; // skip if tempdir is unavailable on this platform
        };
        if let Some(path_str) = dir.path().to_str() {
            assert!(validate_mcp_path(path_str).is_ok());
        }
    }

    #[test]
    fn null_byte_is_rejected() {
        let result = validate_mcp_path("/tmp/foo\0bar");
        let contains_null_byte = result.is_err_and(|e| e.contains("null byte"));
        assert!(contains_null_byte);
    }

    #[test]
    fn etc_prefix_is_rejected() {
        let result = validate_mcp_path("/etc/hosts");
        let matches_msg = result
            .is_err_and(|msg| msg.contains("not permitted") || msg.contains("does not exist"));
        assert!(matches_msg);
    }

    #[test]
    fn ssh_segment_is_rejected() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_owned());
        let ssh_path = format!("{home}/.ssh");
        if std::path::Path::new(&ssh_path).exists() {
            let result = validate_mcp_path(&ssh_path);
            let contains_ssh = result.is_err_and(|msg| msg.contains(".ssh"));
            assert!(contains_ssh);
        }
    }

    #[test]
    fn nonexistent_path_is_rejected() {
        let result =
            validate_mcp_path("/tmp/this-path-should-not-exist-papertowel-test-12345");
        assert!(result.is_err());
    }

    #[test]
    fn nested_project_under_home_passes() {
        let Ok(dir) = TempDir::new() else {
            return; // skip if tempdir is unavailable on this platform
        };
        let _ = fs::write(dir.path().join("main.rs"), "fn main() {}");
        if let Some(path_str) = dir.path().to_str() {
            assert!(validate_mcp_path(path_str).is_ok());
        }
    }
}