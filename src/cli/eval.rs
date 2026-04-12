use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Args;
use serde::Serialize;

use super::OutputFormat;
use super::scan::collect_findings_for_root;

#[derive(Debug, Args)]
pub struct EvalArgs {
    #[arg(default_value = "tests/fixtures")]
    pub path: String,

    #[arg(long, value_enum, default_value = "text")]
    pub format: OutputFormat,

    /// Use mixed-content aggregation while evaluating fixtures.
    #[arg(long, default_value_t = false)]
    pub mixed: bool,
}

#[derive(Debug, Serialize)]
struct EvalRepoResult {
    repo: String,
    expected_ai: bool,
    predicted_ai: bool,
    findings: usize,
}

#[derive(Debug, Serialize)]
struct EvalReport {
    root: String,
    repos_evaluated: usize,
    true_positive: usize,
    true_negative: usize,
    false_positive: usize,
    false_negative: usize,
    precision: f64,
    recall: f64,
    accuracy: f64,
    results: Vec<EvalRepoResult>,
}

pub fn handle(args: &EvalArgs) -> Result<()> {
    let root = PathBuf::from(&args.path);
    let dirs = list_fixture_repos(&root)?;

    let mut results = Vec::new();
    let mut tp = 0_usize;
    let mut tn = 0_usize;
    let mut fp = 0_usize;
    let mut fn_ = 0_usize;

    for dir in dirs {
        let collection = collect_findings_for_root(&dir, args.mixed)?;
        let predicted_ai = !collection.findings.is_empty();
        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_owned();
        let expected_ai = !name.contains("clean");

        match (expected_ai, predicted_ai) {
            (true, true) => tp += 1,
            (false, false) => tn += 1,
            (false, true) => fp += 1,
            (true, false) => fn_ += 1,
        }

        results.push(EvalRepoResult {
            repo: name,
            expected_ai,
            predicted_ai,
            findings: collection.findings.len(),
        });
    }

    results.sort_by(|a, b| a.repo.cmp(&b.repo));

    let precision = ratio(tp, tp + fp);
    let recall = ratio(tp, tp + fn_);
    let accuracy = ratio(tp + tn, tp + tn + fp + fn_);

    let report = EvalReport {
        root: root.to_string_lossy().into_owned(),
        repos_evaluated: results.len(),
        true_positive: tp,
        true_negative: tn,
        false_positive: fp,
        false_negative: fn_,
        precision,
        recall,
        accuracy,
        results,
    };

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    match args.format {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&report)?;
            writeln!(out, "{json}")?;
        }
        OutputFormat::Text | OutputFormat::GithubActions | OutputFormat::Sarif => {
            writeln!(out, "Evaluation report for {}", report.root)?;
            writeln!(out, " repos: {}", report.repos_evaluated)?;
            writeln!(
                out,
                " confusion matrix: TP={} TN={} FP={} FN={}",
                report.true_positive,
                report.true_negative,
                report.false_positive,
                report.false_negative
            )?;
            writeln!(out, " precision: {:.2}", report.precision)?;
            writeln!(out, " recall: {:.2}", report.recall)?;
            writeln!(out, " accuracy: {:.2}", report.accuracy)?;
        }
    }

    Ok(())
}

fn list_fixture_repos(root: &Path) -> Result<Vec<PathBuf>> {
    let entries = std::fs::read_dir(root)?;
    let mut repos = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            repos.push(path);
        }
    }
    repos.sort();
    Ok(repos)
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        return 0.0;
    }

    let numerator_u32 = u32::try_from(numerator)
        .ok()
        .map_or(u32::MAX, |value| value);
    let denominator_u32 = u32::try_from(denominator)
        .ok()
        .map_or(u32::MAX, |value| if value == 0 { u32::MAX } else { value });

    f64::from(numerator_u32) / f64::from(denominator_u32)
}
