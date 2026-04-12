use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Args;
use serde::Serialize;

use super::scan::collect_findings_for_root;
use super::{OutputFormat, SeverityArg};
use crate::cli::report::build_summary;
use crate::learning::StyleBaseline;

#[derive(Debug, Args)]
pub struct CalibrateArgs {
    #[arg(default_value = ".")]
    pub path: String,

    #[arg(long, value_enum, default_value = "text")]
    pub format: OutputFormat,

    #[arg(long)]
    pub output: Option<String>,

    #[arg(long, default_value_t = false)]
    pub apply: bool,

    /// Use mixed-content aggregation while calibrating.
    #[arg(long, default_value_t = false)]
    pub mixed: bool,
}

#[derive(Debug, Serialize)]
struct CalibrationReport {
    path: String,
    files_scanned: usize,
    findings_total: usize,
    by_severity: std::collections::HashMap<String, usize>,
    by_category: std::collections::HashMap<String, usize>,
    ai_probability: f32,
    recommendations: CalibrationRecommendations,
}

#[derive(Debug, Serialize)]
struct CalibrationRecommendations {
    severity_minimum: String,
    comment_high_density_threshold: f32,
    rationale: Vec<String>,
}

pub fn handle(args: &CalibrateArgs) -> Result<()> {
    let root = PathBuf::from(&args.path);
    let collection = collect_findings_for_root(&root, args.mixed)?;
    let summary = build_summary(&collection.findings);
    let baseline = StyleBaseline::load(&root).ok().flatten();

    let high = summary.by_severity.get("HIGH").copied().unwrap_or(0);
    let medium = summary.by_severity.get("MED").copied().unwrap_or(0);

    let minimum = if high == 0 && medium <= 1 {
        SeverityArg::High
    } else {
        SeverityArg::Medium
    };

    let comment_threshold = baseline
        .as_ref()
        .map_or(0.45_f32, StyleBaseline::comment_density_threshold);

    let mut rationale = vec![
        "Recommendations are derived from observed finding distribution in the target repo"
            .to_owned(),
    ];
    if baseline.is_some() {
        rationale
            .push("Learned baseline was used to calibrate comment density threshold".to_owned());
    } else {
        rationale
            .push("No learned baseline found; using default comment density threshold".to_owned());
    }

    let recommendations = CalibrationRecommendations {
        severity_minimum: format!("{minimum:?}").to_lowercase(),
        comment_high_density_threshold: comment_threshold,
        rationale,
    };

    let report = CalibrationReport {
        path: root.to_string_lossy().into_owned(),
        files_scanned: collection.files_scanned,
        findings_total: summary.total_findings,
        by_severity: summary.by_severity,
        by_category: summary.by_category,
        ai_probability: summary.ai_probability,
        recommendations,
    };

    if args.apply {
        let output = output_path(&root, args.output.as_deref());
        write_recommendations(&output, &report.recommendations)?;
    }

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    match args.format {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&report)?;
            writeln!(out, "{json}")?;
        }
        OutputFormat::Text | OutputFormat::GithubActions | OutputFormat::Sarif => {
            writeln!(out, "Calibration report for {}", report.path)?;
            writeln!(out, " files scanned: {}", report.files_scanned)?;
            writeln!(out, " findings: {}", report.findings_total)?;
            writeln!(
                out,
                " ai probability: {:.0}%",
                report.ai_probability * 100.0
            )?;
            writeln!(
                out,
                " recommended severity.minimum: {}",
                report.recommendations.severity_minimum
            )?;
            writeln!(
                out,
                " recommended comments.high_density_threshold: {:.2}",
                report.recommendations.comment_high_density_threshold
            )?;
            if args.apply {
                writeln!(
                    out,
                    " wrote calibration: {}",
                    output_path(&root, args.output.as_deref()).display()
                )?;
            }
        }
    }

    Ok(())
}

fn output_path(root: &Path, cli_output: Option<&str>) -> PathBuf {
    cli_output.map_or_else(
        || root.join(".papertowel").join("calibration.toml"),
        PathBuf::from,
    )
}

fn write_recommendations(path: &Path, recs: &CalibrationRecommendations) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let toml = toml::to_string_pretty(recs)?;
    fs::write(path, toml)?;
    Ok(())
}
