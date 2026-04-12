//!
//! Inspired by [vibescore](https://github.com/stef41/vibescore).

use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Result;
use clap::Args;

use crate::config::resolve_config;
use crate::detection::grading::{Grade, GradeReport};

use super::OutputFormat;
use super::scan::collect_findings_for_root;

#[derive(Debug, Args)]
pub struct GradeArgs {
    #[arg(default_value = ".")]
    pub path: String,

    #[arg(long, value_enum, default_value = "text")]
    pub format: OutputFormat,

    /// Minimum passing grade (fails CI if below).
    #[arg(long, value_name = "GRADE")]
    pub min_grade: Option<String>,

    /// Exit with code 1 if grade is below minimum.
    #[arg(long, default_value_t = false)]
    pub ci: bool,

    /// Emit detailed per-category contribution details.
    #[arg(long, default_value_t = false)]
    pub explain: bool,

    /// Use mixed-content aggregation when collecting findings.
    #[arg(long, default_value_t = false)]
    pub mixed: bool,
}

pub fn handle(args: &GradeArgs) -> Result<()> {
    let start = Instant::now();
    let root = PathBuf::from(&args.path);
    let _ = resolve_config(&root)?;

    let collection = collect_findings_for_root(&root, args.mixed)?;

    // Saturate at u64::MAX (584 million years) — effectively unbounded
    let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    let report =
        GradeReport::from_findings(&collection.findings, collection.files_scanned, duration_ms);

    let stdout = io::stdout();
    let use_color = std::io::IsTerminal::is_terminal(&stdout);
    let mut out = BufWriter::new(stdout.lock());

    match args.format {
        OutputFormat::Text => {
            write_grade_report(&mut out, &report, &root, use_color, args.explain)?;
        }
        OutputFormat::Json => write_json_report(&mut out, &report)?,
        OutputFormat::GithubActions | OutputFormat::Sarif => {
            write_grade_report(&mut out, &report, &root, false, args.explain)?;
        }
    }

    // CI gate: fail if below minimum grade
    if let Some(ref min_str) = args.min_grade {
        let min_grade: Grade = min_str
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid --min-grade: {e}"))?;
        if !report.overall_grade.meets_minimum(min_grade) {
            eprintln!(
                "Grade {} does not meet minimum {}",
                report.overall_grade, min_grade
            );
            std::process::exit(1);
        }
    }

    Ok(())
}

fn write_grade_report(
    out: &mut impl Write,
    report: &GradeReport,
    root: &Path,
    use_color: bool,
    explain: bool,
) -> io::Result<()> {
    let reset = if use_color { "\x1b[0m" } else { "" };
    let bold = if use_color { "\x1b[1m" } else { "" };
    let dim = if use_color { "\x1b[2m" } else { "" };

    // Header
    writeln!(out)?;
    writeln!(
        out,
        "{bold}Slop Check{reset}  v{}",
        env!("CARGO_PKG_VERSION")
    )?;
    writeln!(out, "{}", "═".repeat(50))?;
    writeln!(out)?;

    // Project info
    let project_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("(unknown)");
    writeln!(out, "  {dim}Project:{reset}   {bold}{project_name}{reset}")?;
    writeln!(out, "  {dim}Files:{reset}     {}", report.files_scanned)?;
    #[expect(
        clippy::cast_precision_loss,
        reason = "scan duration in ms fits u32, no meaningful precision loss"
    )]
    let duration_secs = report.scan_duration_ms as f64 / 1000.0;
    writeln!(out, "  {dim}Scanned in{reset} {duration_secs:.2}s")?;
    writeln!(out)?;

    // Category table
    writeln!(out, "┌────────────────────────┬────────┬───────┐")?;
    writeln!(
        out,
        "│ {bold}Category{reset}               │ {bold}Score{reset}  │ {bold}Grade{reset} │"
    )?;
    writeln!(out, "├────────────────────────┼────────┼───────┤")?;

    for cat_score in &report.categories {
        if cat_score.finding_count == 0 {
            continue;
        }

        let cat_label = format!("{:<22}", cat_score.category.label());
        let score_str = format!("{:>6.1}", cat_score.normalized_score);
        let grade = cat_score.grade;
        let grade_color = if use_color { grade.ansi_color() } else { "" };

        writeln!(
            out,
            "│ {cat_label} │ {score_str} │ {grade_color}{bold}{grade:>5}{reset} │"
        )?;
    }

    writeln!(out, "├────────────────────────┼────────┼───────┤")?;

    // Overall row
    let overall_color = if use_color {
        report.overall_grade.ansi_color()
    } else {
        ""
    };
    writeln!(
        out,
        "│ {bold}Overall Slop{reset}           │ {:>6.1} │ {overall_color}{bold}{:>5}{reset} │",
        report.overall_score, report.overall_grade
    )?;
    writeln!(out, "└────────────────────────┴────────┴───────┘")?;
    writeln!(out)?;

    // Interpretation
    let interpretation = match report.overall_grade {
        Grade::APlus | Grade::A | Grade::AMinus => "[CLEAN] Low AI fingerprint presence.",
        Grade::BPlus | Grade::B | Grade::BMinus => "[OK] Some AI fingerprints detected.",
        Grade::CPlus | Grade::C | Grade::CMinus => "[WARN] Moderate AI fingerprint presence.",
        Grade::DPlus | Grade::D | Grade::DMinus => "[ALERT] High AI fingerprint presence.",
        Grade::F => "[FAIL] Very high AI fingerprint presence.",
    };
    writeln!(out, "  {interpretation}")?;

    if report.total_findings > 0 {
        writeln!(out)?;
        writeln!(
            out,
            "  {dim}Run `papertowel scan` for detailed findings.{reset}"
        )?;
    }

    if explain {
        writeln!(out)?;
        writeln!(out, "  {bold}Explainability{reset}")?;
        for category in report.categories.iter().filter(|c| c.finding_count > 0) {
            writeln!(
                out,
                "  - {}: {} finding(s), weighted contribution {:.1}",
                category.category,
                category.finding_count,
                category.normalized_score * category.category.weight(),
            )?;
        }
    }

    writeln!(out)?;

    Ok(())
}

fn write_json_report(out: &mut impl Write, report: &GradeReport) -> io::Result<()> {
    let json = serde_json::to_string_pretty(&report).map_err(io::Error::other)?;
    writeln!(out, "{json}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grade_args_defaults() {
        let args = GradeArgs {
            path: ".".to_owned(),
            format: OutputFormat::Text,
            min_grade: None,
            ci: false,
            explain: false,
            mixed: false,
        };
        assert_eq!(args.path, ".");
        assert!(!args.ci);
    }
}
