//! `papertowel-mcp` — MCP server that exposes papertowel scan and scrub
//! capabilities as tools consumable by LLM clients (e.g. Claude Desktop,
//! Cursor, Continue.dev).
//!
//! # Transport
//!
//! Uses the MCP stdio transport: each message is framed with an
//! LSP-style `Content-Length` header so that clients can parse
//! variable-length JSON payloads:
//!
//! ```text
//! Content-Length: <n>\r\n
//! \r\n
//! <n bytes of UTF-8 JSON>
//! ```
//!
//! # Tools
//!
//! | Tool | Description |
//! |------|-------------|
//! | `papertowel_scan` | Scan a path for AI-fingerprint findings |
//! | `papertowel_scrub` | Dry-run scrub: show what would be changed |

use std::fmt::Write as _;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{debug, error, info, instrument, warn};

// ─── JSON-RPC 2.0 types ───────────────────────────────────────────────────────

/// An incoming JSON-RPC 2.0 request or notification.
#[derive(Debug, Deserialize)]
struct IncomingMessage {
    jsonrpc: String,
    /// Absent for notifications.
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

/// An outgoing JSON-RPC 2.0 response.
#[derive(Debug, Serialize)]
struct Response {
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
const ERR_PARSE: i32 = -32700;
const ERR_INVALID_REQ: i32 = -32600;
const ERR_METHOD_NOT_FOUND: i32 = -32601;
const ERR_INVALID_PARAMS: i32 = -32602;
const ERR_INTERNAL: i32 = -32603;

impl Response {
    #[expect(
        clippy::missing_const_for_fn,
        reason = "serde_json::Value is not const-constructible"
    )]
    fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    fn err(id: Value, code: i32, message: impl Into<String>) -> Self {
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

// ─── MCP protocol constants ───────────────────────────────────────────────────

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "papertowel";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

// ─── Entry point ─────────────────────────────────────────────────────────────

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
                // Write a parse error response with null id.
                let resp = Response::err(Value::Null, ERR_PARSE, format!("read error: {e}"));
                let _ = write_response(&resp, &mut writer);
            }
        }
    }
}

// ─── I/O helpers ─────────────────────────────────────────────────────────────

/// Read one framed message from `reader`.
///
/// Returns `Ok(None)` on EOF, `Ok(Some(json_string))` on success.
fn read_message(reader: &mut impl BufRead) -> Result<Option<String>> {
    // Read headers until blank line.
    let mut content_length: Option<usize> = None;

    loop {
        let mut header = String::new();
        let n = reader
            .read_line(&mut header)
            .context("reading header line")?;
        if n == 0 {
            return Ok(None); // EOF
        }
        let header = header.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break; // blank separator line
        }
        // Header format: `Name: value`
        if let Some(value) = header.strip_prefix("Content-Length:") {
            let trimmed = value.trim();
            content_length = Some(
                trimmed
                    .parse::<usize>()
                    .context("parsing Content-Length value")?,
            );
        }
        // Ignore other headers (e.g. Content-Type).
    }

    let len = content_length.context("no Content-Length header")?;
    let mut buf = vec![0u8; len];
    reader
        .read_exact(&mut buf)
        .context("reading message body")?;
    Ok(Some(
        String::from_utf8(buf).context("message body is not UTF-8")?,
    ))
}

/// Serialise `resp` and write it as a framed MCP message.
fn write_response(resp: &Response, writer: &mut impl Write) -> Result<()> {
    let body = serde_json::to_string(resp).context("serialising response")?;
    write!(writer, "Content-Length: {}\r\n\r\n{}", body.len(), body).context("writing response")?;
    writer.flush().context("flushing response")
}

// ─── Dispatch ─────────────────────────────────────────────────────────────────

/// Parse raw JSON and dispatch to the appropriate handler.
#[instrument(skip_all, fields(raw))]
fn handle_raw(raw: &str, writer: &mut impl Write) {
    let msg: IncomingMessage = match serde_json::from_str(raw) {
        Ok(m) => m,
        Err(e) => {
            let resp = Response::err(Value::Null, ERR_PARSE, format!("invalid JSON: {e}"));
            let _ = write_response(&resp, writer);
            return;
        }
    };

    if msg.jsonrpc != "2.0" {
        if let Some(id) = msg.id {
            let resp = Response::err(id, ERR_INVALID_REQ, "jsonrpc must be \"2.0\"");
            let _ = write_response(&resp, writer);
        }
        return;
    }

    // Notifications (no id) are processed but never get a response.
    let is_notification = msg.id.is_none();

    let result: Result<Value> = match msg.method.as_str() {
        "initialize" => Ok(handle_initialize(msg.params.as_ref())),
        "tools/list" => Ok(handle_tools_list()),
        "tools/call" => handle_tools_call(msg.params.as_ref()),
        "ping" => Ok(json!({})),
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
            let code = if e.to_string().starts_with("method not found") {
                ERR_METHOD_NOT_FOUND
            } else if e.to_string().starts_with("invalid params") {
                ERR_INVALID_PARAMS
            } else {
                ERR_INTERNAL
            };
            Response::err(id, code, e.to_string())
        }
    };

    if let Err(e) = write_response(&resp, writer) {
        error!(error = %e, "failed to write response");
    }
}

// ─── Method handlers ──────────────────────────────────────────────────────────

fn handle_initialize(_params: Option<&Value>) -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": SERVER_NAME,
            "version": SERVER_VERSION
        }
    })
}

fn handle_tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "papertowel_scan",
                "description": "Scan a file or directory for AI-generated code fingerprints. Returns a list of findings with severity, category, and suggested fixes.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute or relative path to the file or directory to scan."
                        },
                        "min_severity": {
                            "type": "string",
                            "enum": ["low", "medium", "high"],
                            "description": "Minimum severity threshold for reported findings. Defaults to 'low'."
                        }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "papertowel_scrub",
                "description": "Dry-run scrub of a file: show what lexical and comment-density changes would be applied to reduce AI fingerprints, without modifying any files.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute or relative path to the source file to analyse."
                        }
                    },
                    "required": ["path"]
                }
            }
        ]
    })
}

fn handle_tools_call(params: Option<&Value>) -> Result<Value> {
    let params = params.ok_or_else(|| anyhow::anyhow!("invalid params: missing params object"))?;

    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("invalid params: missing tool name"))?;

    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    match name {
        "papertowel_scan" => call_scan(&args),
        "papertowel_scrub" => call_scrub(&args),
        unknown => Err(anyhow::anyhow!(
            "method not found: unknown tool '{unknown}'"
        )),
    }
}

// ─── Tool implementations ─────────────────────────────────────────────────────

/// Run the papertowel scan pipeline against a path and return findings as text.
fn call_scan(args: &Value) -> Result<Value> {
    let raw_path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("invalid params: 'path' is required"))?;

    let min_severity_str = args
        .get("min_severity")
        .and_then(Value::as_str)
        .unwrap_or("low");

    let min_severity = parse_severity(min_severity_str)?;

    let path = PathBuf::from(raw_path);
    if !path.exists() {
        return Ok(tool_error(format!("path does not exist: {raw_path}")));
    }

    // Collect files to scan.
    let files = collect_files(&path);
    if files.is_empty() {
        return Ok(tool_text("No analysable source files found."));
    }

    let mut all_findings = Vec::new();
    for file in &files {
        let ext = file
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        let lang = papertowel::detection::language::LanguageKind::from_extension(ext);

        if lang.is_analysable() {
            run_detector_into(
                &mut all_findings,
                papertowel::scrubber::lexical::detect_file(file),
            );
            run_detector_into(
                &mut all_findings,
                papertowel::scrubber::comments::detect_file(file),
            );
            run_detector_into(
                &mut all_findings,
                papertowel::scrubber::structure::detect_file_for_language(file, lang),
            );
            run_detector_into(
                &mut all_findings,
                papertowel::scrubber::tests::detect_file_for_language(file, lang),
            );
            if lang == papertowel::detection::language::LanguageKind::Rust {
                run_detector_into(
                    &mut all_findings,
                    papertowel::scrubber::idiom_mismatch::detect_file(file),
                );
            }
        }

        if matches!(
            ext,
            "rs" | "py" | "go" | "ts" | "tsx" | "cs" | "md" | "toml" | "yaml" | "yml" | "txt"
        ) {
            run_detector_into(
                &mut all_findings,
                papertowel::scrubber::prompt::detect_file(file),
            );
        }

        if ext == "md" {
            run_detector_into(
                &mut all_findings,
                papertowel::scrubber::readme::detect_file(file),
            );
        }
    }

    // Filter by severity.
    all_findings.retain(|f: &papertowel::detection::finding::Finding| {
        severity_value(f.severity) >= severity_value(min_severity)
    });

    if all_findings.is_empty() {
        return Ok(tool_text(format!(
            "No findings at or above '{min_severity_str}' severity."
        )));
    }

    // Render as text.
    let mut out = String::new();
    for f in &all_findings {
        let _ = writeln!(
            out,
            "[{:?}] {} \u{2014} {} ({:?})\n  {}",
            f.severity,
            f.id,
            f.file_path.display(),
            f.category,
            f.description
        );
        if let Some(suggestion) = &f.suggestion {
            let _ = writeln!(out, "  Suggestion: {suggestion}");
        }
        out.push('\n');
    }
    let _ = writeln!(out, "{} finding(s) total.", all_findings.len());

    Ok(tool_text(out))
}

/// Dry-run scrub: report what lexical transforms would change.
fn call_scrub(args: &Value) -> Result<Value> {
    let raw_path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("invalid params: 'path' is required"))?;

    let path = PathBuf::from(raw_path);
    if !path.exists() {
        return Ok(tool_error(format!("path does not exist: {raw_path}")));
    }
    if !path.is_file() {
        return Ok(tool_error(
            "scrub requires a single file path, not a directory",
        ));
    }

    // Run lexical and comment detectors to see what would change.
    let mut findings = Vec::new();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    let lang = papertowel::detection::language::LanguageKind::from_extension(ext);

    run_detector_into(
        &mut findings,
        papertowel::scrubber::lexical::detect_file(&path),
    );
    run_detector_into(
        &mut findings,
        papertowel::scrubber::comments::detect_file(&path),
    );
    if lang.is_analysable() {
        run_detector_into(
            &mut findings,
            papertowel::scrubber::structure::detect_file_for_language(&path, lang),
        );
    }

    if findings.is_empty() {
        return Ok(tool_text(format!(
            "No AI fingerprints detected in {}.",
            path.display()
        )));
    }

    let mut out = format!(
        "Dry-run scrub for {} — {} potential change(s):\n\n",
        path.display(),
        findings.len()
    );
    for f in &findings {
        let _ = writeln!(
            out,
            "\u{2022} [{:?}] {}: {}",
            f.severity, f.id, f.description
        );
        if let Some(s) = &f.suggestion {
            let _ = writeln!(out, "  \u{2192} {s}");
        }
    }

    Ok(tool_text(out))
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Collect all source files under `path` (recurses into directories).
fn collect_files(path: &std::path::Path) -> Vec<PathBuf> {
    if path.is_file() {
        return vec![path.to_path_buf()];
    }
    walkdir::WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e: walkdir::Result<walkdir::DirEntry>| e.ok())
        .filter(|e| e.path().is_file())
        .map(|e| e.path().to_path_buf())
        .collect()
}

/// Append any successfully produced findings; log and discard errors.
fn run_detector_into(
    findings: &mut Vec<papertowel::detection::finding::Finding>,
    result: Result<
        Vec<papertowel::detection::finding::Finding>,
        papertowel::domain::errors::PapertowelError,
    >,
) {
    match result {
        Ok(mut f) => findings.append(&mut f),
        Err(e) => debug!(error = %e, "detector error (skipped)"),
    }
}

/// Parse a severity string into a `Severity` value.
fn parse_severity(s: &str) -> Result<papertowel::detection::finding::Severity> {
    match s {
        "low" => Ok(papertowel::detection::finding::Severity::Low),
        "medium" => Ok(papertowel::detection::finding::Severity::Medium),
        "high" => Ok(papertowel::detection::finding::Severity::High),
        other => Err(anyhow::anyhow!(
            "invalid params: unknown severity '{other}'; expected low/medium/high"
        )),
    }
}

/// Comparable integer for a severity level.
const fn severity_value(s: papertowel::detection::finding::Severity) -> u8 {
    match s {
        papertowel::detection::finding::Severity::Low => 0,
        papertowel::detection::finding::Severity::Medium => 1,
        papertowel::detection::finding::Severity::High => 2,
    }
}

/// Build a successful MCP tool-call result containing a single text block.
fn tool_text(text: impl Into<String>) -> Value {
    json!({
        "content": [{ "type": "text", "text": text.into() }]
    })
}

/// Build a successful MCP tool-call result that signals a tool-level error.
fn tool_error(message: impl Into<String>) -> Value {
    json!({
        "content": [{ "type": "text", "text": message.into() }],
        "isError": true
    })
}
