#![expect(
    clippy::multiple_crate_versions,
    reason = "transitive dependency graph currently includes duplicate versions"
)]

//! `papertowel-mcp` — MCP server that exposes papertowel scan and scrub
//! capabilities as tools consumable by LLM clients (e.g. Claude Desktop,
//! Cursor, Continue.dev).
//!
//! # Transport
//!
//! Implements the MCP stdio transport (spec `2025-11-25`). Each message is a
//! single UTF-8 JSON object followed by a newline (`\n`). Embedded newlines
//! are not permitted inside a message.

use std::io::{self, Write};

use anyhow::Result;
use serde_json::Value;
use tracing::{debug, error, info, instrument, warn};

mod path_guard;
mod protocol;
mod tools;
mod transport;

use protocol::{
    ERR_INVALID_REQ, ERR_PARSE, IncomingMessage, Response, handle_initialize, method_result_code,
};
use tools::{handle_tools_call, handle_tools_list};
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

    info!("papertowel-mcp starting");

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
                handle_raw(&raw, &mut writer);
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
fn handle_raw(raw: &str, writer: &mut impl Write) {
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

    // Notifications (no id) are processed but never get a response.
    let is_notification = msg.id.is_none();
    let method_name = msg.method.clone();

    let result: Result<Value> = match msg.method.as_str() {
        "initialize" => Ok(handle_initialize(msg.params.as_ref())),
        "tools/list" => Ok(handle_tools_list()),
        "tools/call" => handle_tools_call(msg.params.as_ref()),
        "ping" => Ok(serde_json::json!({})),
        // Notifications
        "notifications/initialized" | "notifications/cancelled" => {
            debug!(method = %msg.method, "notification received");
            return; // no response
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
            Response::err(id, code, e.to_string())
        }
    };

    write_response_observed(writer, &resp, "method_result", Some(method_name.as_str()));
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::fs;

    use serde_json::json;
    use tempfile::TempDir;

    use crate::path_guard::validate_mcp_path;
    use crate::protocol::handle_initialize;
    use crate::tools::handle_tools_list;

    #[test]
    fn protocol_surface_for_initialize_and_tools_list_is_stable() {
        let init = handle_initialize(Some(&json!({ "protocolVersion": "2025-11-25" })));

        assert_eq!(
            init.get("protocolVersion"),
            Some(&json!("2025-11-25")),
            "initialize should negotiate and return the current protocol version"
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
        let tools = tools_list
            .get("tools")
            .and_then(serde_json::Value::as_array)
            .expect("tools/list should return a tools array");

        for (expected_name, read_only, idempotent) in [
            ("papertowel_scan", true, true),
            ("papertowel_scrub", true, true),
            ("papertowel_grade", true, true),
            ("papertowel_cleanup_assess", false, true),
            ("papertowel_cleanup_status", true, true),
            ("papertowel_cleanup_apply", false, false),
        ] {
            let tool = tools
                .iter()
                .find(|tool| tool.get("name") == Some(&json!(expected_name)))
                .expect("expected tool should be present in tools/list");

            assert_eq!(
                tool.get("annotations")
                    .and_then(|ann| ann.get("readOnlyHint")),
                Some(&json!(read_only))
            );
            assert_eq!(
                tool.get("annotations")
                    .and_then(|ann| ann.get("destructiveHint")),
                Some(&json!(false))
            );
            assert_eq!(
                tool.get("annotations")
                    .and_then(|ann| ann.get("idempotentHint")),
                Some(&json!(idempotent))
            );
            assert_eq!(
                tool.get("annotations")
                    .and_then(|ann| ann.get("openWorldHint")),
                Some(&json!(false))
            );
        }
    }

    #[test]
    fn valid_project_path_passes() {
        let dir = TempDir::new().expect("tempdir");
        let result = validate_mcp_path(dir.path().to_str().expect("utf8 path"));
        assert!(result.is_ok(), "a normal temp dir should pass: {result:?}");
    }

    #[test]
    fn null_byte_is_rejected() {
        let result = validate_mcp_path("/tmp/foo\0bar");
        assert!(result.is_err());
        assert!(result.expect_err("err").contains("null byte"));
    }

    #[test]
    fn etc_prefix_is_rejected() {
        // /etc/hosts exists on both Linux and macOS (/etc → /private/etc on macOS).
        let result = validate_mcp_path("/etc/hosts");
        assert!(result.is_err());
        let msg = result.expect_err("err");
        assert!(
            msg.contains("not permitted") || msg.contains("does not exist"),
            "unexpected msg: {msg}"
        );
    }

    #[test]
    fn ssh_segment_is_rejected() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_owned());
        let ssh_path = format!("{home}/.ssh");
        if std::path::Path::new(&ssh_path).exists() {
            let result = validate_mcp_path(&ssh_path);
            assert!(result.is_err());
            let msg = result.expect_err("err");
            assert!(msg.contains(".ssh"), "msg: {msg}");
        }
    }

    #[test]
    fn nonexistent_path_is_rejected() {
        let result = validate_mcp_path("/tmp/this-path-should-not-exist-papertowel-test-12345");
        assert!(result.is_err());
    }

    #[test]
    fn nested_project_under_home_passes() {
        let dir = TempDir::new().expect("tempdir");
        fs::write(dir.path().join("main.rs"), "fn main() {}").expect("write");
        let result = validate_mcp_path(dir.path().to_str().expect("utf8 path"));
        assert!(result.is_ok());
    }
}
