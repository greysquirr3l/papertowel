//! `papertowel-mcp` — MCP server that exposes papertowel scan and scrub
//! capabilities as tools consumable by LLM clients (e.g. Claude Desktop,
//! Cursor, Continue.dev).
//!
//! # Transport
//!
//! Implements the MCP stdio transport (spec `2025-11-25`). Each message is a
//! single UTF-8 JSON object followed by a newline (`\n`). Embedded newlines
//! are not permitted inside a message.
//!
//! ```text
//! {"jsonrpc":"2.0","id":1,"method":"initialize", ...}\n
//! {"jsonrpc":"2.0","id":1,"result":{...}}\n
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

const PROTOCOL_VERSION: &str = "2025-11-25";
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

/// Read one newline-delimited JSON message from `reader`.
///
/// Blank lines are skipped. Returns `Ok(None)` on EOF, `Ok(Some(line))` on
/// success. Per the MCP 2025-11-25 stdio transport spec, each message is a
/// single JSON object on its own line with no embedded newlines.
fn read_message(reader: &mut impl BufRead) -> Result<Option<String>> {
    loop {
        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
            .context("reading message line")?;
        if n == 0 {
            return Ok(None); // EOF
        }
        let trimmed = line.trim_end_matches(['\r', '\n']).to_owned();
        if !trimmed.is_empty() {
            return Ok(Some(trimmed));
        }
        // Skip blank lines between messages.
    }
}

/// Serialise `resp` as a compact single-line JSON object followed by `\n`.
///
/// Per the MCP 2025-11-25 stdio transport spec, each message must be a single
/// newline-terminated JSON object with no embedded newlines.
fn write_response(resp: &Response, writer: &mut impl Write) -> Result<()> {
    let body = serde_json::to_string(resp).context("serialising response")?;
    writeln!(writer, "{body}").context("writing response")?;
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

fn handle_initialize(params: Option<&Value>) -> Value {
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
            "tools": {}
        },
        "serverInfo": {
            "name": SERVER_NAME,
            "title": "papertowel MCP Server",
            "version": SERVER_VERSION
        },
        "instructions": "Use papertowel_scan to detect AI-generated code fingerprints in a file or directory. Use papertowel_scrub for a dry-run view of suggested changes without modifying any files."
    })
}

fn handle_tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "papertowel_scan",
                "title": "AI Fingerprint Scanner",
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
                "title": "AI Fingerprint Dry-Run Scrubber",
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

    let path = match validate_mcp_path(raw_path) {
        Ok(p) => p,
        Err(msg) => return Ok(tool_error(msg)),
    };
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

    let path = match validate_mcp_path(raw_path) {
        Ok(p) => p,
        Err(msg) => return Ok(tool_error(msg)),
    };
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

/// Validate that `raw_path` is safe for the MCP server to operate on.
///
/// Rejects:
/// - Paths containing null bytes (potential injection).
/// - Paths that canonicalize to well-known sensitive system directories
///   (`/etc`, `/proc`, `/sys`, `/dev`) or common secret-bearing home
///   sub-directories (`.ssh`, `.gnupg`, `.aws`, `.config/gcloud`).
///
/// Returns the canonicalized [`PathBuf`] on success.
fn validate_mcp_path(raw_path: &str) -> Result<PathBuf, String> {
    const DENIED_PREFIXES: &[&str] = &[
        "/etc",
        "/private/etc", // macOS: /etc is a symlink to /private/etc
        "/proc",
        "/sys",
        "/dev",
        "/private/tmp/../etc", // paranoia
    ];
    const DENIED_SEGMENTS: &[&str] = &[
        ".ssh",
        ".gnupg",
        ".pgp",
        ".aws",
        ".azure",
        ".config/gcloud",
        ".kube",
        "Library/Keychains",
        "Library/Credentials",
    ];

    // Null-byte check.
    if raw_path.contains('\0') {
        return Err("path contains a null byte".to_owned());
    }

    let path = PathBuf::from(raw_path);

    // Canonicalize to resolve `..` and symlinks before the sensitive-prefix check.
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("path is invalid or does not exist: {e}"))?;

    for denied in DENIED_PREFIXES {
        if canonical.starts_with(denied) {
            return Err(format!(
                "scanning '{denied}' is not permitted by the MCP server"
            ));
        }
    }

    let canonical_str = canonical.to_string_lossy();
    for segment in DENIED_SEGMENTS {
        if canonical_str.contains(segment) {
            return Err(format!(
                "path contains sensitive segment '{segment}'; scanning is not permitted"
            ));
        }
    }

    Ok(canonical)
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::validate_mcp_path;

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
        // Could be "not permitted" (prefix matched) or "does not exist" on unusual systems.
        assert!(
            msg.contains("not permitted") || msg.contains("does not exist"),
            "unexpected msg: {msg}"
        );
    }

    #[test]
    fn ssh_segment_is_rejected() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_owned());
        let ssh_path = format!("{home}/.ssh");
        // Only test if the directory actually exists so canonicalize succeeds.
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
