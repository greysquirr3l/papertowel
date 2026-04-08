use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Args;
use indicatif::{ProgressBar, ProgressStyle};
use walkdir::WalkDir;

use super::{OutputFormat, SeverityArg};
use crate::cli::report::{
    build_summary, write_github_actions_report, write_json_report, write_text_report,
};
use crate::config::{build_ignore_matcher, is_ignored, load_config};
use crate::detection::finding::{Finding, Severity};
use crate::detection::language::LanguageKind;
use crate::scrubber::{
    comments, idiom_mismatch, lexical, maintenance, metadata, name_credibility, promotion, readme,
    structure, tests, workflow,
};

#[derive(Debug, Args)]
pub struct ScanArgs {
    pub path: String,
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
    #[arg(long, value_enum)]
    pub severity: Option<SeverityArg>,
    /// Exit with code 1 if any findings at or above this severity are found.
    /// Useful for gating CI pipelines.
    #[arg(long, value_name = "SEVERITY", value_enum)]
    pub fail_on: Option<SeverityArg>,
}

pub fn handle(args: &ScanArgs) -> Result<()> {
    let root = PathBuf::from(&args.path);
    let config = load_config(&root).unwrap_or_default();
    let ignore = build_ignore_matcher(&root, &config)?;

    let min_severity = args.severity.map(|s| match s {
        SeverityArg::Low => Severity::Low,
        SeverityArg::Medium => Severity::Medium,
        SeverityArg::High => Severity::High,
    });

    let mut findings: Vec<Finding> = Vec::new();

    // ── File-level detectors ─────────────────────────────────────────────────
    let files: Vec<PathBuf> = WalkDir::new(&root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            !ignore
                .as_ref()
                .is_some_and(|m| is_ignored(m, &root, e.path(), false))
        })
        .map(|e| e.path().to_path_buf())
        .collect();

    let bar = ProgressBar::new(files.len() as u64);
    #[expect(
        clippy::literal_string_with_formatting_args,
        reason = "indicatif progress bar template syntax, not a format string"
    )]
    let style = ProgressStyle::default_bar()
        .template("{spinner:.green} [{bar:40}] {pos}/{len} {msg}")
        .unwrap_or_else(|_| ProgressStyle::default_bar());
    bar.set_style(style);

    for path in &files {
        bar.inc(1);
        bar.set_message(path.to_string_lossy().into_owned());
        let _ = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_lowercase();
        run_file_detectors(path, &mut findings);
    }

    bar.finish_and_clear();

    // ── Repo-level detectors ─────────────────────────────────────────────────
    if is_git_repo(&root) {
        run_repo_detectors(&root, &mut findings);
    }

    // ── Severity filtering ───────────────────────────────────────────────────
    if let Some(min) = min_severity {
        findings.retain(|f| f.severity >= min);
    }

    let summary = build_summary(&findings);
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    match args.format {
        OutputFormat::Text => write_text_report(&mut out, &findings, &summary)?,
        OutputFormat::Json => write_json_report(&mut out, &findings, &summary)?,
        OutputFormat::GithubActions => write_github_actions_report(&mut out, &findings, &summary)?,
    }

    // CI gate: exit 1 if any finding is at or above the --fail-on threshold.
    if let Some(fail_sev) = args.fail_on {
        let threshold = match fail_sev {
            SeverityArg::Low => Severity::Low,
            SeverityArg::Medium => Severity::Medium,
            SeverityArg::High => Severity::High,
        };
        if findings.iter().any(|f| f.severity >= threshold) {
            std::process::exit(1);
        }
    }

    Ok(())
}

fn run_file_detectors(path: &Path, findings: &mut Vec<Finding>) {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    let lang = LanguageKind::from_extension(ext);

    if lang.is_analysable() {
        run_detector(findings, || lexical::detect_file(path));
        run_detector(findings, || comments::detect_file(path));
        run_detector(findings, || structure::detect_file_for_language(path, lang));
        run_detector(findings, || tests::detect_file_for_language(path, lang));
        if lang == LanguageKind::Rust {
            run_detector(findings, || idiom_mismatch::detect_file(path));
        }
    }

    if ext == "md" {
        run_detector(findings, || readme::detect_file(path));
    }

    if matches!(
        ext,
        "rs" | "py" | "go" | "ts" | "tsx" | "cs" | "md" | "toml" | "yaml" | "yml" | "txt"
    ) {
        run_detector(findings, || crate::scrubber::prompt::detect_file(path));
    }
}

fn run_repo_detectors(root: &Path, findings: &mut Vec<Finding>) {
    run_detector(findings, || crate::scrubber::commit_pattern::detect_repo(root));
    run_detector(findings, || workflow::detect_repo(root));
    run_detector(findings, || promotion::detect_repo(root));
    run_detector(findings, || metadata::detect_repo(root));
    run_detector(findings, || maintenance::detect_repo(root));
    run_detector(findings, || name_credibility::detect_repo(root));
}

fn run_detector(
    findings: &mut Vec<Finding>,
    f: impl FnOnce() -> Result<Vec<Finding>, crate::domain::errors::PapertowelError>,
) {
    match f() {
        Ok(mut found) => findings.append(&mut found),
        Err(e) => tracing::warn!("detector error: {e}"),
    }
}

fn is_git_repo(path: &Path) -> bool {
    path.join(".git").exists()
}
