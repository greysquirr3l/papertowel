use std::fmt::Write as _;
use std::path::PathBuf;

use anyhow::Result;
use serde_json::{Value, json};
use tracing::debug;

use crate::path_guard::validate_mcp_path;

/// Files larger than this are skipped by the recipe scanner to avoid I/O waste.
const MAX_RECIPE_SCAN_BYTES: u64 = 2 * 1024 * 1024;

pub fn handle_tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "papertowel_scan",
                "title": "AI Fingerprint Scanner",
                "description": "Scan a file or directory for AI-generated code fingerprints. Returns a list of findings with severity, category, and suggested fixes.",
                "annotations": {
                    "readOnlyHint": true,
                    "destructiveHint": false,
                    "idempotentHint": true,
                    "openWorldHint": false
                },
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
                "description": "Dry-run scrub of a file: show what lexical, comment-density, structural, and recipe-based changes would be applied to reduce AI fingerprints, without modifying any files.",
                "annotations": {
                    "readOnlyHint": true,
                    "destructiveHint": false,
                    "idempotentHint": true,
                    "openWorldHint": false
                },
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
            },
            {
                "name": "papertowel_grade",
                "title": "AI Fingerprint Grade",
                "description": "Score a file or directory A+ through F for overall AI fingerprint presence. Returns the overall grade, per-category breakdown, and total finding count. Optionally include per-category contribution details with explain=true.",
                "annotations": {
                    "readOnlyHint": true,
                    "destructiveHint": false,
                    "idempotentHint": true,
                    "openWorldHint": false
                },
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute or relative path to the file or directory to grade."
                        },
                        "explain": {
                            "type": "boolean",
                            "description": "Include per-category score and finding count in the output. Defaults to false."
                        }
                    },
                    "required": ["path"]
                }
            }
        ]
    })
}

pub fn handle_tools_call(params: Option<&Value>) -> Result<Value> {
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
        "papertowel_scan" => Ok(call_scan(&args)),
        "papertowel_scrub" => Ok(call_scrub(&args)),
        "papertowel_grade" => Ok(call_grade(&args)),
        unknown => Err(anyhow::anyhow!(
            "method not found: unknown tool '{unknown}'"
        )),
    }
}

/// Load a `RecipeMatcher` for the project root that contains `path`.
///
/// Returns `None` if recipes cannot be loaded or compiled (errors are logged at
/// `debug` level so the scan still proceeds with structural findings).
fn load_recipe_matcher(path: &std::path::Path) -> Option<papertowel::recipe::RecipeMatcher> {
    let project_root = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .map_or_else(|| path.to_path_buf(), PathBuf::from)
    };
    let loader = papertowel::recipe::RecipeLoader::new(Some(project_root));
    match loader.load_all() {
        Ok(recipes) => papertowel::recipe::RecipeMatcher::compile(recipes).ok(),
        Err(e) => {
            debug!(error = %e, "failed to load recipes (skipped)");
            None
        }
    }
}

/// Run the papertowel scan pipeline against a path and return findings as text.
fn call_scan(args: &Value) -> Value {
    let Some(raw_path) = args.get("path").and_then(Value::as_str) else {
        return tool_error("invalid arguments: 'path' is required");
    };

    let min_severity_str = args
        .get("min_severity")
        .and_then(Value::as_str)
        .unwrap_or("low");

    let min_severity = match parse_severity(min_severity_str) {
        Ok(severity) => severity,
        Err(message) => return tool_error(message),
    };

    let path = match validate_mcp_path(raw_path) {
        Ok(p) => p,
        Err(msg) => return tool_error(msg),
    };
    if !path.exists() {
        return tool_error(format!("path does not exist: {raw_path}"));
    }

    // Collect files to scan.
    let files = collect_files(&path);
    if files.is_empty() {
        return tool_text("No analysable source files found.");
    }

    // Load recipe matcher from the path's project root (best-effort).
    let recipe_matcher = load_recipe_matcher(&path);

    let mut all_findings = Vec::new();
    for file in &files {
        scan_file_into(&mut all_findings, file, recipe_matcher.as_ref());
    }

    // Run repository-level detectors when scanning a directory that is a git repo.
    if path.is_dir() && path.join(".git").exists() {
        run_detector_into(
            &mut all_findings,
            papertowel::scrubber::commit_pattern::detect_repo(&path),
        );
        run_detector_into(
            &mut all_findings,
            papertowel::scrubber::architecture::detect_repo(&path),
        );
        run_detector_into(
            &mut all_findings,
            papertowel::scrubber::workflow::detect_repo(&path),
        );
        run_detector_into(
            &mut all_findings,
            papertowel::scrubber::promotion::detect_repo(&path),
        );
        run_detector_into(
            &mut all_findings,
            papertowel::scrubber::metadata::detect_repo(&path),
        );
        run_detector_into(
            &mut all_findings,
            papertowel::scrubber::maintenance::detect_repo(&path),
        );
        run_detector_into(
            &mut all_findings,
            papertowel::scrubber::name_credibility::detect_repo(&path),
        );
    }

    // Filter by severity.
    all_findings.retain(|f: &papertowel::detection::finding::Finding| {
        severity_value(f.severity) >= severity_value(min_severity)
    });

    if all_findings.is_empty() {
        return tool_text(format!(
            "No findings at or above '{min_severity_str}' severity."
        ));
    }

    // Render as text.
    let mut out = String::new();
    for f in &all_findings {
        let _ = writeln!(
            out,
            "[{:?}] {} — {} ({:?})\n  {}",
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

    tool_text(out)
}

/// Dry-run scrub: report what lexical transforms would change.
fn call_scrub(args: &Value) -> Value {
    let Some(raw_path) = args.get("path").and_then(Value::as_str) else {
        return tool_error("invalid arguments: 'path' is required");
    };

    let path = match validate_mcp_path(raw_path) {
        Ok(p) => p,
        Err(msg) => return tool_error(msg),
    };
    if !path.exists() {
        return tool_error(format!("path does not exist: {raw_path}"));
    }
    if !path.is_file() {
        return tool_error("scrub requires a single file path, not a directory");
    }

    // Run lexical, comment, and structural detectors to see what would change.
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

    // Recipe-based detection on the single file.
    let recipe_matcher = load_recipe_matcher(&path);
    if let Some(ref matcher) = recipe_matcher
        && path
            .metadata()
            .map_or(true, |m| m.len() <= MAX_RECIPE_SCAN_BYTES)
        && let Ok(content) = std::fs::read_to_string(&path)
    {
        match matcher.scan_file(&path, &content) {
            Ok(mut recipe_findings) => findings.append(&mut recipe_findings),
            Err(e) => debug!(error = %e, "recipe scan error (skipped)"),
        }
    }

    if findings.is_empty() {
        return tool_text(format!(
            "No AI fingerprints detected in {}.",
            path.display()
        ));
    }

    let mut out = format!(
        "Dry-run scrub for {} — {} potential change(s):\n\n",
        path.display(),
        findings.len()
    );
    for f in &findings {
        let _ = writeln!(out, "• [{:?}] {}: {}", f.severity, f.id, f.description);
        if let Some(s) = &f.suggestion {
            let _ = writeln!(out, "  → {s}");
        }
    }

    tool_text(out)
}

/// Grade a path for overall AI fingerprint presence and return a letter score.
fn call_grade(args: &Value) -> Value {
    let Some(raw_path) = args.get("path").and_then(Value::as_str) else {
        return tool_error("invalid arguments: 'path' is required");
    };

    let explain = args
        .get("explain")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let path = match validate_mcp_path(raw_path) {
        Ok(p) => p,
        Err(msg) => return tool_error(msg),
    };
    if !path.exists() {
        return tool_error(format!("path does not exist: {raw_path}"));
    }

    let start = std::time::Instant::now();
    let collection = match papertowel::cli::scan::collect_findings_for_root(&path, false) {
        Ok(c) => c,
        Err(e) => return tool_error(format!("scan failed: {e}")),
    };
    let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

    let report = papertowel::detection::grading::GradeReport::from_findings(
        &collection.findings,
        collection.files_scanned,
        duration_ms,
    );

    let mut out = format!(
        "Grade: {} (score {:.1})\nFiles: {}  Findings: {}\n",
        report.overall_grade, report.overall_score, report.files_scanned, report.total_findings,
    );

    if explain {
        out.push('\n');
        for cat in &report.categories {
            if cat.finding_count > 0 {
                let _ = writeln!(
                    out,
                    "  {}: {} ({} finding(s))",
                    cat.category, cat.grade, cat.finding_count
                );
            }
        }
    }

    tool_text(out)
}

/// Run all detectors for a single file and append findings to `out`.
fn scan_file_into(
    out: &mut Vec<papertowel::detection::finding::Finding>,
    file: &std::path::Path,
    recipe_matcher: Option<&papertowel::recipe::RecipeMatcher>,
) {
    let ext = file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    let lang = papertowel::detection::language::LanguageKind::from_extension(ext);

    if lang.is_analysable() {
        run_detector_into(out, papertowel::scrubber::lexical::detect_file(file));
        run_detector_into(out, papertowel::scrubber::comments::detect_file(file));
        run_detector_into(
            out,
            papertowel::scrubber::structure::detect_file_for_language(file, lang),
        );
        run_detector_into(
            out,
            papertowel::scrubber::tests::detect_file_for_language(file, lang),
        );
        if lang == papertowel::detection::language::LanguageKind::Rust {
            run_detector_into(out, papertowel::scrubber::idiom_mismatch::detect_file(file));
        }
    }

    if papertowel::scrubber::security::is_supported_source_extension(ext) {
        run_detector_into(out, papertowel::scrubber::security::detect_file(file));
    }

    if matches!(
        ext,
        "rs" | "py"
            | "go"
            | "ts"
            | "tsx"
            | "cs"
            | "zig"
            | "cpp"
            | "cc"
            | "cxx"
            | "hpp"
            | "hxx"
            | "md"
            | "toml"
            | "yaml"
            | "yml"
            | "txt"
    ) {
        run_detector_into(out, papertowel::scrubber::prompt::detect_file(file));
    }

    if ext == "md" {
        run_detector_into(out, papertowel::scrubber::readme::detect_file(file));
    }

    // Recipe-based detection: runs on any text file under 2 MiB.
    if let Some(matcher) = recipe_matcher
        && file
            .metadata()
            .map_or(true, |m| m.len() <= MAX_RECIPE_SCAN_BYTES)
        && let Ok(content) = std::fs::read_to_string(file)
    {
        match matcher.scan_file(file, &content) {
            Ok(mut recipe_findings) => out.append(&mut recipe_findings),
            Err(e) => debug!(error = %e, file = %file.display(), "recipe scan error (skipped)"),
        }
    }
}

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
fn parse_severity(
    s: &str,
) -> std::result::Result<papertowel::detection::finding::Severity, String> {
    match s {
        "low" => Ok(papertowel::detection::finding::Severity::Low),
        "medium" => Ok(papertowel::detection::finding::Severity::Medium),
        "high" => Ok(papertowel::detection::finding::Severity::High),
        other => Err(format!(
            "invalid arguments: unknown severity '{other}'; expected low/medium/high"
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
