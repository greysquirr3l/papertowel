use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command as ProcessCommand;

use anyhow::Result;
use serde::Serialize;
use serde_json::{Value, json};
use std::str::FromStr;
use tracing::debug;

use crate::path_guard::validate_mcp_path;

mod args;
mod output;

use args::{bool_arg, optional_str_arg, required_str_arg};
use output::{tool_error, tool_text};

/// Files larger than this are skipped by the recipe scanner to avoid I/O waste.
const MAX_RECIPE_SCAN_BYTES: u64 = 2 * 1024 * 1024;

pub fn handle_tools_list() -> Value {
    json!({
        "tools": [
            tool_scan_definition(),
            tool_scrub_definition(),
            tool_grade_definition(),
            tool_cleanup_assess_definition(),
            tool_cleanup_status_definition(),
            tool_cleanup_apply_definition(),
        ]
    })
}

fn tool_scan_definition() -> Value {
    json!({
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
    })
}

fn tool_scrub_definition() -> Value {
    json!({
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
    })
}

fn tool_grade_definition() -> Value {
    json!({
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
    })
}

fn tool_cleanup_assess_definition() -> Value {
    json!({
        "name": "papertowel_cleanup_assess",
        "title": "Cleanup Assess",
        "description": "Build a cleanup assessment report with track routing, confidence classes, deferred queue, and validation plan. Persists artifacts under .papertowel/cleanup.",
        "annotations": {
            "readOnlyHint": false,
            "destructiveHint": false,
            "idempotentHint": true,
            "openWorldHint": false
        },
        "inputSchema": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or relative path to assess. Defaults to current directory."
                },
                "tracks": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": [
                            "deduplication",
                            "type_consolidation",
                            "dead_code",
                            "circular_dependencies",
                            "type_strengthening",
                            "error_handling",
                            "deprecated_and_ai_artifacts"
                        ]
                    },
                    "description": "Optional subset of cleanup tracks to assess."
                },
                "state_dir": {
                    "type": "string",
                    "description": "Optional cleanup state directory override."
                }
            }
        }
    })
}

fn tool_cleanup_status_definition() -> Value {
    json!({
        "name": "papertowel_cleanup_status",
        "title": "Cleanup Status",
        "description": "Read persisted cleanup status, deferred queue, evidence gaps, and summary trend deltas from .papertowel/cleanup.",
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
                    "description": "Absolute or relative path used to resolve default cleanup state directory. Defaults to current directory."
                },
                "state_dir": {
                    "type": "string",
                    "description": "Optional cleanup state directory override."
                }
            }
        }
    })
}

fn tool_cleanup_apply_definition() -> Value {
    json!({
        "name": "papertowel_cleanup_apply",
        "title": "Cleanup Apply",
        "description": "Select applicable cleanup findings using strict policy gates and run validation commands from the cleanup report.",
        "annotations": {
            "readOnlyHint": false,
            "destructiveHint": true,
            "idempotentHint": false,
            "openWorldHint": true
        },
        "inputSchema": {
            "type": "object",
            "properties": {
                "report": {
                    "type": "string",
                    "description": "Path to cleanup report JSON produced by papertowel_cleanup_assess."
                },
                "min_confidence": {
                    "type": "string",
                    "enum": ["low", "medium", "high"],
                    "description": "Minimum confidence class required for apply selection. Defaults to high."
                },
                "max_risk": {
                    "type": "string",
                    "enum": ["low", "medium", "high"],
                    "description": "Maximum risk level allowed for apply selection. Defaults to low."
                },
                "allow_tracks": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": [
                            "deduplication",
                            "type_consolidation",
                            "dead_code",
                            "circular_dependencies",
                            "type_strengthening",
                            "error_handling",
                            "deprecated_and_ai_artifacts"
                        ]
                    },
                    "description": "Optional allow-list of tracks for apply selection."
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "When true, do not persist apply artifacts. Defaults to false."
                },
                "ci": {
                    "type": "boolean",
                    "description": "When true, validation failures always return tool error."
                },
                "state_dir": {
                    "type": "string",
                    "description": "Optional cleanup state directory override."
                }
            },
            "required": ["report"]
        }
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
        "papertowel_cleanup_assess" => Ok(call_cleanup_assess(&args)),
        "papertowel_cleanup_status" => Ok(call_cleanup_status(&args)),
        "papertowel_cleanup_apply" => Ok(call_cleanup_apply(&args)),
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
    let matcher = papertowel::recipe::loader::load_recipe_matcher_for_path(path);
    if matcher.is_none() {
        debug!("failed to load recipes (skipped)");
    }
    matcher
}

fn load_lexical_matcher(
    path: &std::path::Path,
) -> std::result::Result<Option<papertowel::scrubber::lexical::LexicalMatcher>, String> {
    let (_, config, _) = papertowel::config::resolve_config(path)
        .map_err(|e| format!("failed to load project config: {e}"))?;

    if !config.detectors.lexical.enabled() {
        return Ok(None);
    }

    let rules = config.detectors.lexical.rules();
    papertowel::scrubber::lexical::LexicalMatcher::from_rules(&rules)
        .map(Some)
        .map_err(|e| format!("failed to build lexical matcher: {e}"))
}

/// Run the papertowel scan pipeline against a path and return findings as text.
fn call_scan(args: &Value) -> Value {
    let raw_path = match required_str_arg(args, "path") {
        Ok(path) => path,
        Err(message) => return tool_error(message),
    };

    let min_severity_str = optional_str_arg(args, "min_severity", "low");

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
    let lexical_matcher = match load_lexical_matcher(&path) {
        Ok(matcher) => matcher,
        Err(message) => return tool_error(message),
    };

    let mut all_findings = Vec::new();
    for file in &files {
        scan_file_into(
            &mut all_findings,
            file,
            recipe_matcher.as_ref(),
            lexical_matcher.as_ref(),
        );
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
    all_findings.retain(|f: &papertowel::detection::finding::Finding| f.severity >= min_severity);

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
    let raw_path = match required_str_arg(args, "path") {
        Ok(path) => path,
        Err(message) => return tool_error(message),
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
    let raw_path = match required_str_arg(args, "path") {
        Ok(path) => path,
        Err(message) => return tool_error(message),
    };

    let explain = bool_arg(args, "explain", false);

    let path = match validate_mcp_path(raw_path) {
        Ok(p) => p,
        Err(msg) => return tool_error(msg),
    };
    if !path.exists() {
        return tool_error(format!("path does not exist: {raw_path}"));
    }

    let start = std::time::Instant::now();
    let collection = match papertowel::cli::scan::collect_findings_for_root(&path, false, false) {
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

#[derive(Debug, Clone, Serialize)]
struct CleanupValidationResult {
    command: String,
    success: bool,
    exit_code: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
struct CleanupBlockedItem {
    id: String,
    track: String,
    reason: String,
}

#[derive(Debug, Clone, Serialize)]
struct CleanupApplyResult {
    report_path: String,
    dry_run: bool,
    approved_count: usize,
    blocked_count: usize,
    blocked: Vec<CleanupBlockedItem>,
    validation: Vec<CleanupValidationResult>,
}

fn call_cleanup_assess(args: &Value) -> Value {
    let raw_path = optional_str_arg(args, "path", ".");
    let path = match validate_mcp_path(raw_path) {
        Ok(p) => p,
        Err(msg) => return tool_error(msg),
    };
    if !path.exists() {
        return tool_error(format!("path does not exist: {raw_path}"));
    }

    let tracks_arg = match optional_string_array_arg(args, "tracks") {
        Ok(v) => v,
        Err(message) => return tool_error(message),
    };
    let tracks = if tracks_arg.is_empty() {
        papertowel::cleanup::CleanupTrack::all().to_vec()
    } else {
        let mut parsed_tracks = Vec::new();
        for track in tracks_arg {
            let parsed = match parse_cleanup_track(&track) {
                Ok(track) => track,
                Err(message) => return tool_error(message),
            };
            parsed_tracks.push(parsed);
        }
        parsed_tracks
    };

    let path_str = path.to_string_lossy();
    let report = papertowel::cleanup::build_assess_report(&path_str, &tracks);

    let state_dir = papertowel::cleanup::resolve_state_dir(
        &report.path,
        args.get("state_dir").and_then(Value::as_str),
    );
    if let Err(error) = papertowel::cleanup::persist_assess_artifacts(&state_dir, &report) {
        return tool_error(format!("failed to persist cleanup artifacts: {error}"));
    }

    serialize_json_text(&report)
}

fn call_cleanup_status(args: &Value) -> Value {
    let raw_path = optional_str_arg(args, "path", ".");
    let path = match validate_mcp_path(raw_path) {
        Ok(p) => p,
        Err(msg) => return tool_error(msg),
    };

    let path_str = path.to_string_lossy();
    let state_dir = papertowel::cleanup::resolve_state_dir(
        &path_str,
        args.get("state_dir").and_then(Value::as_str),
    );

    match papertowel::cleanup::read_status_report(&state_dir) {
        Ok(status) => serialize_json_text(&status),
        Err(error) => tool_error(format!("failed to read cleanup status: {error}")),
    }
}

fn call_cleanup_apply(args: &Value) -> Value {
    let raw_report = match required_str_arg(args, "report") {
        Ok(path) => path,
        Err(message) => return tool_error(message),
    };
    let report_path = match validate_mcp_path(raw_report) {
        Ok(p) => p,
        Err(msg) => return tool_error(msg),
    };
    if !report_path.exists() {
        return tool_error(format!("path does not exist: {raw_report}"));
    }

    let report_json = match std::fs::read_to_string(&report_path) {
        Ok(content) => content,
        Err(error) => return tool_error(format!("failed to read report JSON: {error}")),
    };

    let report: papertowel::cleanup::CleanupReport = match serde_json::from_str(&report_json) {
        Ok(report) => report,
        Err(error) => return tool_error(format!("invalid report JSON: {error}")),
    };

    let min_confidence =
        match parse_confidence_class(optional_str_arg(args, "min_confidence", "high")) {
            Ok(value) => value,
            Err(message) => return tool_error(message),
        };

    let max_risk = match parse_cleanup_risk(optional_str_arg(args, "max_risk", "low")) {
        Ok(value) => value,
        Err(message) => return tool_error(message),
    };

    let allow_tracks_arg = match optional_string_array_arg(args, "allow_tracks") {
        Ok(v) => v,
        Err(message) => return tool_error(message),
    };
    let allowed_tracks = if allow_tracks_arg.is_empty() {
        report.tracks.clone()
    } else {
        let mut parsed_tracks = Vec::new();
        for track in allow_tracks_arg {
            let parsed = match parse_cleanup_track(&track) {
                Ok(track) => track,
                Err(message) => return tool_error(message),
            };
            parsed_tracks.push(parsed);
        }
        parsed_tracks
    };

    let mut policy = papertowel::cleanup::CleanupApplyPolicy::strict_default(&allowed_tracks);
    policy.min_confidence = min_confidence;
    policy.max_risk = max_risk;

    let selection = papertowel::cleanup::select_applicable_findings(&report, &policy);

    let validation = run_validation_commands(&report.validation_plan.commands);
    let any_validation_failed = validation.iter().any(|v| !v.success);

    let dry_run = bool_arg(args, "dry_run", false);
    let ci = bool_arg(args, "ci", false);

    if !dry_run {
        let state_dir = papertowel::cleanup::resolve_state_dir(
            &report.path,
            args.get("state_dir").and_then(Value::as_str),
        );
        if let Err(error) = papertowel::cleanup::persist_assess_artifacts(&state_dir, &report) {
            return tool_error(format!("failed to persist cleanup artifacts: {error}"));
        }
    }

    let result = CleanupApplyResult {
        report_path: report_path.to_string_lossy().into_owned(),
        dry_run,
        approved_count: selection.approved.len(),
        blocked_count: selection.blocked.len(),
        blocked: selection
            .blocked
            .iter()
            .map(|blocked| CleanupBlockedItem {
                id: blocked.finding.id.clone(),
                track: cleanup_track_name(blocked.finding.track).to_owned(),
                reason: blocked.reason.clone(),
            })
            .collect(),
        validation,
    };

    if any_validation_failed && (!dry_run || ci) {
        return tool_error("cleanup apply validation failed");
    }

    serialize_json_text(&result)
}

fn serialize_json_text<T: Serialize>(value: &T) -> Value {
    match serde_json::to_string_pretty(value) {
        Ok(text) => tool_text(text),
        Err(error) => tool_error(format!("failed to serialize JSON output: {error}")),
    }
}

fn optional_string_array_arg(args: &Value, name: &str) -> std::result::Result<Vec<String>, String> {
    let Some(raw) = args.get(name) else {
        return Ok(Vec::new());
    };

    let Some(items) = raw.as_array() else {
        return Err(format!(
            "invalid arguments: '{name}' must be an array of strings"
        ));
    };

    let mut parsed = Vec::new();
    for item in items {
        let Some(value) = item.as_str() else {
            return Err(format!(
                "invalid arguments: '{name}' must be an array of strings"
            ));
        };
        parsed.push(value.to_owned());
    }
    Ok(parsed)
}

fn parse_cleanup_track(s: &str) -> std::result::Result<papertowel::cleanup::CleanupTrack, String> {
    match s {
        "deduplication" => Ok(papertowel::cleanup::CleanupTrack::Deduplication),
        "type_consolidation" => Ok(papertowel::cleanup::CleanupTrack::TypeConsolidation),
        "dead_code" => Ok(papertowel::cleanup::CleanupTrack::DeadCode),
        "circular_dependencies" => Ok(papertowel::cleanup::CleanupTrack::CircularDependencies),
        "type_strengthening" => Ok(papertowel::cleanup::CleanupTrack::TypeStrengthening),
        "error_handling" => Ok(papertowel::cleanup::CleanupTrack::ErrorHandling),
        "deprecated_and_ai_artifacts" => {
            Ok(papertowel::cleanup::CleanupTrack::DeprecatedAndAiArtifacts)
        }
        _ => Err(format!(
            "invalid arguments: unsupported cleanup track '{s}'"
        )),
    }
}

fn parse_confidence_class(
    s: &str,
) -> std::result::Result<papertowel::cleanup::CleanupConfidenceClass, String> {
    match s {
        "low" => Ok(papertowel::cleanup::CleanupConfidenceClass::Low),
        "medium" => Ok(papertowel::cleanup::CleanupConfidenceClass::Medium),
        "high" => Ok(papertowel::cleanup::CleanupConfidenceClass::High),
        _ => Err(format!(
            "invalid arguments: unsupported min_confidence '{s}'; expected low/medium/high"
        )),
    }
}

fn parse_cleanup_risk(s: &str) -> std::result::Result<papertowel::cleanup::CleanupRisk, String> {
    match s {
        "low" => Ok(papertowel::cleanup::CleanupRisk::Low),
        "medium" => Ok(papertowel::cleanup::CleanupRisk::Medium),
        "high" => Ok(papertowel::cleanup::CleanupRisk::High),
        _ => Err(format!(
            "invalid arguments: unsupported max_risk '{s}'; expected low/medium/high"
        )),
    }
}

const fn cleanup_track_name(track: papertowel::cleanup::CleanupTrack) -> &'static str {
    match track {
        papertowel::cleanup::CleanupTrack::Deduplication => "deduplication",
        papertowel::cleanup::CleanupTrack::TypeConsolidation => "type_consolidation",
        papertowel::cleanup::CleanupTrack::DeadCode => "dead_code",
        papertowel::cleanup::CleanupTrack::CircularDependencies => "circular_dependencies",
        papertowel::cleanup::CleanupTrack::TypeStrengthening => "type_strengthening",
        papertowel::cleanup::CleanupTrack::ErrorHandling => "error_handling",
        papertowel::cleanup::CleanupTrack::DeprecatedAndAiArtifacts => {
            "deprecated_and_ai_artifacts"
        }
    }
}

fn run_validation_commands(commands: &[String]) -> Vec<CleanupValidationResult> {
    commands
        .iter()
        .map(|command| {
            let mut shell = shell_command_for(command);
            let status = shell.status();
            status.map_or_else(
                |_| CleanupValidationResult {
                    command: command.clone(),
                    success: false,
                    exit_code: None,
                },
                |status| CleanupValidationResult {
                    command: command.clone(),
                    success: status.success(),
                    exit_code: status.code(),
                },
            )
        })
        .collect()
}

#[cfg(windows)]
fn shell_command_for(command: &str) -> ProcessCommand {
    let mut cmd = ProcessCommand::new("cmd");
    cmd.arg("/C").arg(command);
    cmd
}

#[cfg(not(windows))]
fn shell_command_for(command: &str) -> ProcessCommand {
    let mut cmd = ProcessCommand::new("sh");
    cmd.arg("-c").arg(command);
    cmd
}

/// Run all detectors for a single file and append findings to `out`.
fn scan_file_into(
    out: &mut Vec<papertowel::detection::finding::Finding>,
    file: &std::path::Path,
    recipe_matcher: Option<&papertowel::recipe::RecipeMatcher>,
    lexical_matcher: Option<&papertowel::scrubber::lexical::LexicalMatcher>,
) {
    let ext = file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    let lang = papertowel::detection::language::LanguageKind::from_extension(ext);

    if lang.is_analysable() {
        if let Some(matcher) = lexical_matcher {
            run_detector_into(
                out,
                matcher.detect_file(
                    file,
                    papertowel::scrubber::lexical::LexicalDetectionConfig::default(),
                ),
            );
        }
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
    papertowel::detection::finding::Severity::from_str(s)
        .map_err(|e| format!("invalid arguments: {e}; expected low/medium/high"))
}
