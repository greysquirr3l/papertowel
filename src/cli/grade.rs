//!
//! Inspired by [vibescore](https://github.com/stef41/vibescore).

use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use clap::Args;
use indicatif::{ProgressBar, ProgressStyle};
use walkdir::WalkDir;

use crate::config::{is_ignored, resolve_config};
use crate::detection::finding::Finding;
use crate::detection::grading::{Grade, GradeReport};
use crate::detection::language::LanguageKind;
use crate::learning::StyleBaseline;
use crate::recipe::loader::RecipeLoader;
use crate::recipe::matcher::RecipeMatcher;
use crate::scrubber::comments::CommentDetectionConfig;
use crate::scrubber::ignore_directives;
use crate::scrubber::{
    comments, idiom_mismatch, lexical, maintenance, metadata, name_credibility, promotion, readme,
    security, structure, tests as scrubber_tests, workflow,
};

use super::OutputFormat;
use super::scan::MAX_RECIPE_SCAN_BYTES;

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
}

pub fn handle(args: &GradeArgs) -> Result<()> {
    let start = Instant::now();
    let root = PathBuf::from(&args.path);
    let (project_root, config, ignore) = resolve_config(&root)?;

    let baseline = StyleBaseline::load(&project_root).ok().flatten();
    let comment_config = baseline
        .as_ref()
        .map_or_else(CommentDetectionConfig::default, |b| {
            CommentDetectionConfig {
                high_density_threshold: b.comment_density_threshold(),
                ..CommentDetectionConfig::default()
            }
        });

    let recipe_matcher = load_recipe_matcher(&project_root);

    let mut findings: Vec<Finding> = Vec::new();

    let files: Vec<PathBuf> = WalkDir::new(&root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            !ignore
                .as_ref()
                .is_some_and(|m| is_ignored(m, &project_root, e.path(), false))
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

        let directives =
            ignore_directives::parse_file(path).unwrap_or_else(|_| ignore_directives::parse(""));
        if directives.ignore_file {
            continue;
        }

        let pre_count = findings.len();
        run_file_detectors(
            path,
            &mut findings,
            comment_config,
            recipe_matcher.as_deref(),
            config.detectors.security,
        );

        if !directives.suppressed_lines.is_empty() {
            let new_findings = findings.split_off(pre_count);
            let filtered = directives.filter_findings(new_findings);
            findings.extend(filtered);
        }
    }

    bar.finish_and_clear();

    if is_git_repo(&root) {
        run_repo_detectors(&root, &mut findings);
    }

    // Saturate at u64::MAX (584 million years) — effectively unbounded
    let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    let report = GradeReport::from_findings(&findings, files.len(), duration_ms);

    let stdout = io::stdout();
    let use_color = std::io::IsTerminal::is_terminal(&stdout);
    let mut out = BufWriter::new(stdout.lock());

    match args.format {
        OutputFormat::Text => write_grade_report(&mut out, &report, &root, use_color)?,
        OutputFormat::Json => write_json_report(&mut out, &report)?,
        OutputFormat::GithubActions | OutputFormat::Sarif => {
            write_grade_report(&mut out, &report, &root, false)?;
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

    writeln!(out)?;

    Ok(())
}

fn write_json_report(out: &mut impl Write, report: &GradeReport) -> io::Result<()> {
    let json = serde_json::to_string_pretty(&report).map_err(io::Error::other)?;
    writeln!(out, "{json}")
}

fn load_recipe_matcher(project_root: &Path) -> Option<Arc<RecipeMatcher>> {
    let loader = RecipeLoader::new(Some(project_root.to_path_buf()));
    match loader.load_all() {
        Ok(recipes) => RecipeMatcher::compile(recipes).ok().map(Arc::new),
        Err(e) => {
            tracing::warn!("failed to load recipes: {e}");
            None
        }
    }
}

fn run_file_detectors(
    path: &Path,
    findings: &mut Vec<Finding>,
    comment_config: CommentDetectionConfig,
    recipe_matcher: Option<&RecipeMatcher>,
    security_enabled: bool,
) {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    let lang = LanguageKind::from_extension(ext);

    if lang.is_analysable() {
        run_detector(findings, || lexical::detect_file(path));
        run_detector(findings, || {
            comments::detect_file_with_config(path, comment_config)
        });
        run_detector(findings, || structure::detect_file_for_language(path, lang));
        run_detector(findings, || {
            scrubber_tests::detect_file_for_language(path, lang)
        });
        if lang == LanguageKind::Rust {
            run_detector(findings, || idiom_mismatch::detect_file(path));
        }
    }

    if security_enabled && security::is_supported_source_extension(ext) {
        run_detector(findings, || security::detect_file(path));
    }

    if let Some(matcher) = recipe_matcher
        && path
            .metadata()
            .map_or(true, |m| m.len() <= MAX_RECIPE_SCAN_BYTES)
        && let Ok(content) = std::fs::read_to_string(path)
    {
        match matcher.scan_file(path, &content) {
            Ok(mut recipe_findings) => findings.append(&mut recipe_findings),
            Err(e) => tracing::warn!("recipe scan error for {}: {e}", path.display()),
        }
    }

    if ext == "md" {
        run_detector(findings, || readme::detect_file(path));
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
        run_detector(findings, || crate::scrubber::prompt::detect_file(path));
    }
}

fn run_repo_detectors(root: &Path, findings: &mut Vec<Finding>) {
    run_detector(findings, || {
        crate::scrubber::commit_pattern::detect_repo(root)
    });
    run_detector(findings, || {
        crate::scrubber::architecture::detect_repo(root)
    });
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
        };
        assert_eq!(args.path, ".");
        assert!(!args.ci);
    }
}
