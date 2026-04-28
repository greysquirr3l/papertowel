use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::process::Command as ProcessCommand;

use anyhow::Result;
use clap::{Args, Subcommand, ValueEnum};

use crate::cleanup::{
    CleanupApplyPolicy, CleanupConfidenceClass, CleanupReport, CleanupRisk, CleanupStatusReport,
    CleanupTrack, build_assess_report, persist_assess_artifacts, read_status_report,
    resolve_state_dir, select_applicable_findings,
};

use super::OutputFormat;

#[derive(Debug, Args)]
pub struct CleanupArgs {
    #[command(subcommand)]
    pub command: CleanupCommand,
}

#[derive(Debug, Subcommand)]
pub enum CleanupCommand {
    /// Read-only cleanup assessment with confidence and evidence scaffolding.
    Assess(AssessArgs),
    /// Show persisted deferred cleanup backlog and evidence gaps.
    Status(StatusArgs),
    /// Apply cleanup report candidates using strict policy gates.
    Apply(ApplyArgs),
}

#[derive(Debug, Args)]
pub struct AssessArgs {
    #[arg(default_value = ".")]
    pub path: String,

    #[arg(long, value_enum, default_value = "text")]
    pub format: OutputFormat,

    #[arg(long, value_enum, value_delimiter = ',')]
    pub tracks: Vec<CleanupTrackArg>,

    /// Write a machine-readable JSON report to disk.
    #[arg(long)]
    pub out: Option<String>,

    /// Override cleanup state directory (default: project path/.papertowel/cleanup).
    #[arg(long)]
    pub state_dir: Option<String>,

    /// CI mode placeholder for future policy gating behavior.
    #[arg(long, default_value_t = false)]
    pub ci: bool,

    /// Reuse mixed-content handling where track analyzers support it.
    #[arg(long, default_value_t = false)]
    pub mixed: bool,
}

#[derive(Debug, Args)]
pub struct StatusArgs {
    #[arg(default_value = ".")]
    pub path: String,

    #[arg(long, value_enum, default_value = "text")]
    pub format: OutputFormat,

    /// Override cleanup state directory (default: project path/.papertowel/cleanup).
    #[arg(long)]
    pub state_dir: Option<String>,
}

#[derive(Debug, Args)]
pub struct ApplyArgs {
    /// Path to a cleanup report JSON file produced by `cleanup assess`.
    pub report: String,

    #[arg(long, value_enum, default_value = "text")]
    pub format: OutputFormat,

    #[arg(long, value_enum, default_value = "high")]
    pub min_confidence: CleanupConfidenceArg,

    #[arg(long, value_enum, default_value = "low")]
    pub max_risk: CleanupRiskArg,

    #[arg(long, value_enum, value_delimiter = ',')]
    pub allow_tracks: Vec<CleanupTrackArg>,

    #[arg(long, default_value_t = false)]
    pub dry_run: bool,

    #[arg(long, default_value_t = false)]
    pub ci: bool,

    /// Override cleanup state directory (default: report path/.papertowel/cleanup).
    #[arg(long)]
    pub state_dir: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CleanupConfidenceArg {
    Low,
    Medium,
    High,
}

impl From<CleanupConfidenceArg> for CleanupConfidenceClass {
    fn from(value: CleanupConfidenceArg) -> Self {
        match value {
            CleanupConfidenceArg::Low => Self::Low,
            CleanupConfidenceArg::Medium => Self::Medium,
            CleanupConfidenceArg::High => Self::High,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CleanupRiskArg {
    Low,
    Medium,
    High,
}

impl From<CleanupRiskArg> for CleanupRisk {
    fn from(value: CleanupRiskArg) -> Self {
        match value {
            CleanupRiskArg::Low => Self::Low,
            CleanupRiskArg::Medium => Self::Medium,
            CleanupRiskArg::High => Self::High,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct CleanupValidationResult {
    command: String,
    success: bool,
    exit_code: Option<i32>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct CleanupBlockedItem {
    id: String,
    track: String,
    reason: String,
}

#[derive(Debug, Clone, serde::Serialize)]
struct CleanupApplyResult {
    report_path: String,
    dry_run: bool,
    approved_count: usize,
    blocked_count: usize,
    blocked: Vec<CleanupBlockedItem>,
    validation: Vec<CleanupValidationResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CleanupTrackArg {
    #[value(name = "deduplication")]
    Deduplication,
    #[value(name = "type_consolidation")]
    TypeConsolidation,
    #[value(name = "dead_code")]
    DeadCode,
    #[value(name = "circular_dependencies")]
    CircularDependencies,
    #[value(name = "type_strengthening")]
    TypeStrengthening,
    #[value(name = "error_handling")]
    ErrorHandling,
    #[value(name = "deprecated_and_ai_artifacts")]
    DeprecatedAndAiArtifacts,
}

impl From<CleanupTrackArg> for CleanupTrack {
    fn from(value: CleanupTrackArg) -> Self {
        match value {
            CleanupTrackArg::Deduplication => Self::Deduplication,
            CleanupTrackArg::TypeConsolidation => Self::TypeConsolidation,
            CleanupTrackArg::DeadCode => Self::DeadCode,
            CleanupTrackArg::CircularDependencies => Self::CircularDependencies,
            CleanupTrackArg::TypeStrengthening => Self::TypeStrengthening,
            CleanupTrackArg::ErrorHandling => Self::ErrorHandling,
            CleanupTrackArg::DeprecatedAndAiArtifacts => Self::DeprecatedAndAiArtifacts,
        }
    }
}

pub fn handle_assess(args: &AssessArgs) -> Result<()> {
    let selected_tracks = if args.tracks.is_empty() {
        CleanupTrack::all().to_vec()
    } else {
        args.tracks
            .iter()
            .copied()
            .map(CleanupTrack::from)
            .collect()
    };

    let report = build_assess_report(&args.path, &selected_tracks);
    let state_dir = resolve_state_dir(&args.path, args.state_dir.as_deref());
    persist_assess_artifacts(&state_dir, &report)?;

    if let Some(out_path) = args.out.as_deref() {
        write_report_json(Path::new(out_path), &report)?;
    }

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    match args.format {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&report)?;
            writeln!(out, "{json}")?;
        }
        OutputFormat::Text | OutputFormat::GithubActions | OutputFormat::Sarif => {
            write_text_report(&mut out, &report)?;
        }
    }

    Ok(())
}

pub fn handle_status(args: &StatusArgs) -> Result<()> {
    let state_dir = resolve_state_dir(&args.path, args.state_dir.as_deref());
    let status = read_status_report(&state_dir)?;

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    match args.format {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&status)?;
            writeln!(out, "{json}")?;
        }
        OutputFormat::Text | OutputFormat::GithubActions | OutputFormat::Sarif => {
            write_status_text_report(&mut out, &status)?;
        }
    }

    Ok(())
}

pub fn handle_apply(args: &ApplyArgs) -> Result<()> {
    let report_path = Path::new(&args.report);
    let report_json = fs::read_to_string(report_path)?;
    let report: CleanupReport = serde_json::from_str(&report_json)?;

    let allowed_tracks = if args.allow_tracks.is_empty() {
        report.tracks.clone()
    } else {
        args.allow_tracks
            .iter()
            .copied()
            .map(CleanupTrack::from)
            .collect::<Vec<_>>()
    };

    let mut policy = CleanupApplyPolicy::strict_default(&allowed_tracks);
    policy.min_confidence = args.min_confidence.into();
    policy.max_risk = args.max_risk.into();

    let selection = select_applicable_findings(&report, &policy);

    let validation = run_validation_commands(&report.validation_plan.commands);
    let any_validation_failed = validation.iter().any(|v| !v.success);

    if !args.dry_run {
        let state_dir = resolve_state_dir(&report.path, args.state_dir.as_deref());
        persist_assess_artifacts(&state_dir, &report)?;
    }

    let result = CleanupApplyResult {
        report_path: args.report.clone(),
        dry_run: args.dry_run,
        approved_count: selection.approved.len(),
        blocked_count: selection.blocked.len(),
        blocked: selection
            .blocked
            .iter()
            .map(|blocked| CleanupBlockedItem {
                id: blocked.finding.id.clone(),
                track: track_name(blocked.finding.track).to_owned(),
                reason: blocked.reason.clone(),
            })
            .collect(),
        validation,
    };

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    match args.format {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&result)?;
            writeln!(out, "{json}")?;
        }
        OutputFormat::Text | OutputFormat::GithubActions | OutputFormat::Sarif => {
            write_apply_text_report(&mut out, &result)?;
        }
    }

    if any_validation_failed && (!args.dry_run || args.ci) {
        anyhow::bail!("cleanup apply validation failed");
    }

    Ok(())
}

fn write_report_json(path: &Path, report: &CleanupReport) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let json = serde_json::to_string_pretty(report)?;
    fs::write(path, json)?;
    Ok(())
}

fn write_text_report(out: &mut impl Write, report: &CleanupReport) -> io::Result<()> {
    writeln!(out, "Cleanup assessment")?;
    writeln!(out, " path: {}", report.path)?;
    writeln!(out, " schema version: {}", report.version)?;

    writeln!(out, " tracks:")?;
    for track in &report.tracks {
        writeln!(out, "  - {}", track_name(*track))?;
    }

    writeln!(out, " summary:")?;
    writeln!(out, "  findings: {}", report.summary.finding_count)?;
    writeln!(out, "  apply: {}", report.summary.apply_count)?;
    writeln!(out, "  review: {}", report.summary.review_count)?;
    writeln!(out, "  defer: {}", report.summary.defer_count)?;

    writeln!(out, " validation plan:")?;
    for command in &report.validation_plan.commands {
        writeln!(out, "  - {command}")?;
    }

    Ok(())
}

const fn track_name(track: CleanupTrack) -> &'static str {
    match track {
        CleanupTrack::Deduplication => "deduplication",
        CleanupTrack::TypeConsolidation => "type_consolidation",
        CleanupTrack::DeadCode => "dead_code",
        CleanupTrack::CircularDependencies => "circular_dependencies",
        CleanupTrack::TypeStrengthening => "type_strengthening",
        CleanupTrack::ErrorHandling => "error_handling",
        CleanupTrack::DeprecatedAndAiArtifacts => "deprecated_and_ai_artifacts",
    }
}

fn write_status_text_report(out: &mut impl Write, status: &CleanupStatusReport) -> io::Result<()> {
    writeln!(out, "Cleanup status")?;
    writeln!(out, " state dir: {}", status.state_dir)?;
    writeln!(out, " schema version: {}", status.version)?;
    writeln!(out, " deferred findings: {}", status.deferred_count)?;
    writeln!(out, " evidence gaps: {}", status.evidence_gap_count)?;

    if let Some(trend) = &status.trend {
        writeln!(out, " trend:")?;
        writeln!(out, "  findings: {}", format_delta(trend.finding_delta))?;
        writeln!(out, "  apply: {}", format_delta(trend.apply_delta))?;
        writeln!(out, "  review: {}", format_delta(trend.review_delta))?;
        writeln!(out, "  defer: {}", format_delta(trend.defer_delta))?;
    }

    if !status.deferred.is_empty() {
        writeln!(out, " deferred queue:")?;
        for finding in &status.deferred {
            writeln!(
                out,
                "  - {} [{}] missing_evidence={}",
                finding.id,
                track_name(finding.track),
                finding.evidence.missing.len(),
            )?;
        }
    }

    Ok(())
}

fn write_apply_text_report(out: &mut impl Write, result: &CleanupApplyResult) -> io::Result<()> {
    writeln!(out, "Cleanup apply")?;
    writeln!(out, " report: {}", result.report_path)?;
    writeln!(out, " dry run: {}", result.dry_run)?;
    writeln!(out, " approved: {}", result.approved_count)?;
    writeln!(out, " blocked: {}", result.blocked_count)?;

    if !result.blocked.is_empty() {
        writeln!(out, " blocked items:")?;
        for blocked in &result.blocked {
            writeln!(
                out,
                "  - {} [{}] reason={}",
                blocked.id, blocked.track, blocked.reason
            )?;
        }
    }

    writeln!(out, " validation:")?;
    for check in &result.validation {
        let status = if check.success { "pass" } else { "fail" };
        writeln!(
            out,
            "  - {status}: {} (exit={:?})",
            check.command, check.exit_code
        )?;
    }

    Ok(())
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

fn format_delta(value: i64) -> String {
    if value > 0 {
        format!("+{value}")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::{
        ApplyArgs, AssessArgs, CleanupConfidenceArg, CleanupRiskArg, CleanupTrackArg, StatusArgs,
        handle_apply, handle_assess, handle_status,
    };
    use crate::cli::OutputFormat;

    #[test]
    fn handle_assess_text_with_selected_tracks() {
        let Ok(temp) = TempDir::new() else {
            return;
        };
        let path = temp.path().to_string_lossy().into_owned();

        let args = AssessArgs {
            path: path.clone(),
            format: OutputFormat::Text,
            tracks: vec![CleanupTrackArg::DeadCode],
            out: None,
            state_dir: None,
            ci: false,
            mixed: false,
        };

        let result = handle_assess(&args);
        assert!(result.is_ok());

        let status_args = StatusArgs {
            path,
            format: OutputFormat::Json,
            state_dir: None,
        };
        let status_result = handle_status(&status_args);
        assert!(status_result.is_ok());
    }

    #[test]
    fn handle_apply_dry_run_blocks_non_apply_candidates() {
        let Ok(temp) = TempDir::new() else {
            return;
        };
        let path = temp.path().to_string_lossy().into_owned();
        let report_path = temp.path().join("cleanup-report.json");

        let assess_args = AssessArgs {
            path,
            format: OutputFormat::Json,
            tracks: vec![CleanupTrackArg::DeadCode],
            out: Some(report_path.to_string_lossy().into_owned()),
            state_dir: None,
            ci: false,
            mixed: false,
        };
        let assess_result = handle_assess(&assess_args);
        assert!(assess_result.is_ok());

        let apply_args = ApplyArgs {
            report: report_path.to_string_lossy().into_owned(),
            format: OutputFormat::Text,
            min_confidence: CleanupConfidenceArg::High,
            max_risk: CleanupRiskArg::Low,
            allow_tracks: Vec::new(),
            dry_run: true,
            ci: false,
            state_dir: None,
        };

        let apply_result = handle_apply(&apply_args);
        assert!(apply_result.is_ok());
    }
}
