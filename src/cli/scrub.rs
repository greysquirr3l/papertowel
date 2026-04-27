use std::collections::BTreeMap;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use clap::Args;
use walkdir::WalkDir;

use crate::cli::scan::collect_findings_for_root;
use crate::config::{is_ignored, resolve_config};
use crate::detection::finding::Severity;
use crate::detection::language::LanguageKind;
use crate::recipe::loader::RecipeLoader;
use crate::recipe::scrubber::RecipeScrubber;
use crate::scrubber::ignore_directives;
use crate::scrubber::safety::{SafetyConfig, SafetyOutcome, check_safety};
use crate::scrubber::{comments, readme};

pub const RECIPE_DETECTOR_NAME: &str = "recipe";

#[derive(Debug, Args)]
pub struct ScrubArgs {
    pub path: String,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long, value_delimiter = ',')]
    pub detectors: Vec<String>,
    /// Treat elevated warnings as hard failures (mirror scan --ci behaviour).
    #[arg(long)]
    pub ci: bool,
    /// Bypass the scrub safety valve even when thresholds would be violated.
    /// Use only for debugging or intentional aggressive transformations.
    #[arg(long)]
    pub allow_unsafe_scrub: bool,
    /// After scrubbing, re-scan and compare scores; exit non-zero in CI mode if score regresses.
    #[arg(long)]
    pub verify: bool,
}

// ── Verification types ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum VerificationStatus {
    Improved,
    Unchanged,
    Regressed,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VerificationMetrics {
    pub findings: usize,
    pub weighted_score: u64,
    pub per_category: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VerificationResult {
    pub status: VerificationStatus,
    pub before: VerificationMetrics,
    pub after: VerificationMetrics,
}

fn run_verification(root: &Path, pre: Option<VerificationMetrics>) -> Result<VerificationResult> {
    let after_scan = collect_findings_for_root(root, false)?;
    let after_metrics = verification_metrics(&after_scan.findings);
    Ok(compare_verification(
        pre.unwrap_or_else(|| VerificationMetrics {
            findings: 0,
            weighted_score: 0,
            per_category: BTreeMap::new(),
        }),
        after_metrics,
    ))
}

fn enforce_ci_guards(
    ci: bool,
    safety_blocked_count: usize,
    verification: Option<&VerificationResult>,
) -> Result<()> {
    if ci && safety_blocked_count > 0 {
        anyhow::bail!(
            "{safety_blocked_count} file{} blocked by the scrub safety valve; \
             use --allow-unsafe-scrub to override",
            if safety_blocked_count == 1 { "" } else { "s" }
        );
    }

    if ci
        && let Some(v) = verification
        && v.status == VerificationStatus::Regressed
    {
        anyhow::bail!(
            "scrub verification failed: weighted score increased from {} to {}",
            v.before.weighted_score,
            v.after.weighted_score
        );
    }

    Ok(())
}

/// Compute severity-weighted score and per-category counts from a finding slice.
fn verification_metrics(findings: &[crate::detection::finding::Finding]) -> VerificationMetrics {
    let weighted_score = findings
        .iter()
        .map(|f| match f.severity {
            Severity::Low => 1u64,
            Severity::Medium => 3,
            Severity::High => 9,
        })
        .sum();
    let mut per_category: BTreeMap<String, usize> = BTreeMap::new();
    for f in findings {
        let key = format!("{:?}", f.category);
        *per_category.entry(key).or_insert(0) += 1;
    }
    VerificationMetrics {
        findings: findings.len(),
        weighted_score,
        per_category,
    }
}

/// Compare before/after metrics and return a `VerificationResult`.
pub fn compare_verification(
    before: VerificationMetrics,
    after: VerificationMetrics,
) -> VerificationResult {
    let status = match before.weighted_score.cmp(&after.weighted_score) {
        std::cmp::Ordering::Greater => VerificationStatus::Improved,
        std::cmp::Ordering::Equal => VerificationStatus::Unchanged,
        std::cmp::Ordering::Less => VerificationStatus::Regressed,
    };
    VerificationResult {
        status,
        before,
        after,
    }
}

#[derive(Debug, Default)]
struct ScrubSummary {
    files_changed: usize,
    recipe_replacements: usize,
    comment_lines_removed: usize,
    readme_lines_removed: usize,
}

struct FileResult {
    path: PathBuf,
    /// Recipe-based replacements applied.
    recipe: Option<usize>,
    /// Over-documentation comment lines removed.
    comments: Option<usize>,
    /// README framework lines removed.
    readme: Option<usize>,
    /// Set when the safety guard reverted an over-aggressive transform.
    safety_blocked: Option<String>,
}

impl FileResult {
    const fn changed(&self) -> bool {
        self.recipe.is_some() || self.comments.is_some() || self.readme.is_some()
    }
}

fn wants_detector(detectors: &[String], name: &str) -> bool {
    if detectors.is_empty() {
        return true;
    }
    // Accept "lexical" as a legacy alias for "recipe" so existing scripts
    // that previously passed --detectors lexical still get recipe replacements.
    let effective = if name == RECIPE_DETECTOR_NAME {
        &[name, "lexical"][..]
    } else {
        &[name][..]
    };
    detectors.iter().any(|d| effective.contains(&d.as_str()))
}

/// Load the recipe scrubber, returning None if loading fails or no patterns exist.
fn load_recipe_scrubber(project_root: &Path) -> Option<Arc<RecipeScrubber>> {
    let loader = RecipeLoader::new(Some(project_root.to_path_buf()));
    match loader.load_all() {
        Ok(recipes) if !recipes.is_empty() => match RecipeScrubber::compile(recipes) {
            Ok(scrubber) if scrubber.has_patterns() => Some(Arc::new(scrubber)),
            Ok(_) => {
                tracing::debug!("no fixable patterns in loaded recipes");
                None
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to compile recipe scrubber");
                None
            }
        },
        Ok(_) => {
            tracing::debug!("no recipes loaded");
            None
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to load recipes for scrubber");
            None
        }
    }
}

pub fn handle(args: &ScrubArgs) -> Result<()> {
    let root = PathBuf::from(&args.path);
    let (project_root, config, ignore) = resolve_config(&root)?;
    let safety_config = SafetyConfig {
        min_size_percent: config.scrubber.min_size_percent,
        max_line_drop_percent: config.scrubber.max_line_drop_percent,
    };
    let effective_safety = if args.allow_unsafe_scrub {
        None
    } else {
        Some(safety_config)
    };

    // Pre-scrub scan snapshot (only collected when --verify is requested).
    let pre_scrub_metrics: Option<VerificationMetrics> = if args.verify {
        let pre_scan = collect_findings_for_root(&root, false)?;
        Some(verification_metrics(&pre_scan.findings))
    } else {
        None
    };

    // Load recipe scrubber once for all files.
    let recipe_scrubber = if wants_detector(&args.detectors, RECIPE_DETECTOR_NAME) {
        load_recipe_scrubber(&project_root)
    } else {
        None
    };

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

    let mut changed_results: Vec<FileResult> = Vec::new();
    let mut summary = ScrubSummary::default();
    let mut safety_blocked_count = 0usize;

    for path in &files {
        // Respect inline ignore-file directives.
        let skip = ignore_directives::parse_file(path)
            .map(|d| d.ignore_file)
            .unwrap_or(false);
        if skip {
            continue;
        }

        let result = apply_transforms(
            path,
            args,
            recipe_scrubber.as_deref(),
            effective_safety.as_ref(),
        );
        if result.safety_blocked.is_some() {
            safety_blocked_count += 1;
        }
        if result.changed() {
            summary.files_changed += 1;
            if let Some(n) = result.recipe {
                summary.recipe_replacements += n;
            }
            if let Some(n) = result.comments {
                summary.comment_lines_removed += n;
            }
            if let Some(n) = result.readme {
                summary.readme_lines_removed += n;
            }
            changed_results.push(result);
        } else if result.safety_blocked.is_some() {
            changed_results.push(result);
        }
    }

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    // ── Verification pass ────────────────────────────────────────────────────
    let verification: Option<VerificationResult> = if args.verify {
        Some(run_verification(&root, pre_scrub_metrics)?)
    } else {
        None
    };

    write_report(
        &mut out,
        &changed_results,
        &summary,
        args.dry_run,
        verification.as_ref(),
    )?;

    enforce_ci_guards(args.ci, safety_blocked_count, verification.as_ref())?;

    Ok(())
}

fn apply_transforms(
    path: &Path,
    args: &ScrubArgs,
    recipe_scrubber: Option<&RecipeScrubber>,
    safety: Option<&SafetyConfig>,
) -> FileResult {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    let lang = LanguageKind::from_extension(ext);

    // Snapshot original content once so we can check safety and revert if needed.
    // Only read when we might actually write (not dry-run, safety active).
    let original_snapshot: Option<String> = if !args.dry_run && safety.is_some() {
        match std::fs::read_to_string(path) {
            Ok(s) => Some(s),
            Err(e) => {
                tracing::debug!(path = %path.display(), "could not snapshot for safety check: {e}");
                None
            }
        }
    } else {
        None
    };

    let mut result = FileResult {
        path: path.to_path_buf(),
        recipe: None,
        comments: None,
        readme: None,
        safety_blocked: None,
    };

    if lang.is_analysable() && wants_detector(&args.detectors, comments::DETECTOR_NAME) {
        match comments::transform_file(path, args.dry_run) {
            Ok(r) if r.changed => result.comments = Some(r.removed_comment_lines),
            Ok(_) => {}
            Err(e) => tracing::warn!(path = %path.display(), "comments transform error: {e}"),
        }
    }

    // Apply recipe-based transforms to all UTF-8 readable files; language gating
    // is intentionally omitted here so Markdown, TOML, and other text formats are
    // covered the same way the scan command covers them.
    if let Some(scrubber) = recipe_scrubber {
        match scrubber.transform_file(path, args.dry_run) {
            Ok(r) if r.changed => result.recipe = Some(r.replacements_applied),
            Ok(_) => {}
            // Downgrade to debug — binary/non-UTF-8 files produce expected failures here.
            Err(e) => tracing::debug!(path = %path.display(), "recipe transform skipped: {e}"),
        }
    }

    if ext == "md" && wants_detector(&args.detectors, readme::DETECTOR_NAME) {
        match readme::transform_file(path, args.dry_run) {
            Ok(r) if r.changed => result.readme = Some(r.removed_lines),
            Ok(_) => {}
            Err(e) => tracing::warn!(path = %path.display(), "readme transform error: {e}"),
        }
    }

    // Safety check: compare original snapshot against current file content.
    // If any transform was over-aggressive, revert to the snapshot.
    if let (Some(original), Some(cfg)) = (&original_snapshot, safety)
        && result.changed()
    {
        match std::fs::read_to_string(path) {
            Ok(current) => match check_safety(original, &current, cfg) {
                SafetyOutcome::Allowed => {}
                SafetyOutcome::Blocked(reason) => {
                    tracing::warn!(
                        path = %path.display(),
                        "safety guard blocked transform: {reason}; reverting"
                    );
                    if let Err(e) = std::fs::write(path, original) {
                        tracing::error!(
                            path = %path.display(),
                            "failed to revert after safety block: {e}"
                        );
                    } else {
                        result.recipe = None;
                        result.comments = None;
                        result.readme = None;
                        result.safety_blocked = Some(reason);
                    }
                }
            },
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    "could not re-read for safety check: {e}"
                );
            }
        }
    }

    result
}

fn write_report(
    out: &mut impl Write,
    results: &[FileResult],
    summary: &ScrubSummary,
    dry_run: bool,
    verification: Option<&VerificationResult>,
) -> io::Result<()> {
    let action = if dry_run { "would change" } else { "changed" };

    for r in results {
        writeln!(out, "{}", r.path.display())?;
        if let Some(n) = r.recipe {
            writeln!(
                out,
                " [recipe] {n} replacement{}",
                if n == 1 { "" } else { "s" }
            )?;
        }
        if let Some(n) = r.comments {
            writeln!(
                out,
                " [comments] {n} comment line{} removed",
                if n == 1 { "" } else { "s" }
            )?;
        }
        if let Some(n) = r.readme {
            writeln!(
                out,
                " [readme] {n} line{} removed",
                if n == 1 { "" } else { "s" }
            )?;
        }
        if let Some(ref reason) = r.safety_blocked {
            writeln!(out, " [safety] blocked — {reason} (original preserved)")?;
        }
        writeln!(out)?;
    }

    let divider = "─".repeat(52);
    writeln!(out, "{divider}")?;
    if summary.files_changed == 0 {
        writeln!(
            out,
            " No changes {}",
            if dry_run { "needed" } else { "made" }
        )?;
    } else {
        write!(
            out,
            " {} file{} {action}",
            summary.files_changed,
            if summary.files_changed == 1 { "" } else { "s" }
        )?;
        if summary.recipe_replacements > 0 {
            write!(out, " · {} recipe", summary.recipe_replacements)?;
        }
        if summary.comment_lines_removed > 0 {
            write!(out, " · {} comments", summary.comment_lines_removed)?;
        }
        if summary.readme_lines_removed > 0 {
            write!(out, " · {} readme", summary.readme_lines_removed)?;
        }
        writeln!(out)?;
    }
    writeln!(out, "{divider}")?;

    // ── Verification section ────────────────────────────────────────
    if let Some(v) = verification {
        let status_label = match v.status {
            VerificationStatus::Improved => "improved",
            VerificationStatus::Unchanged => "unchanged",
            VerificationStatus::Regressed => "REGRESSED",
        };
        writeln!(out, " verification: {status_label}")?;
        writeln!(
            out,
            "  before  findings={} score={}",
            v.before.findings, v.before.weighted_score
        )?;
        writeln!(
            out,
            "  after   findings={} score={}",
            v.after.findings, v.after.weighted_score
        )?;
        // Per-category deltas for categories that changed.
        let all_keys: std::collections::BTreeSet<&String> = v
            .before
            .per_category
            .keys()
            .chain(v.after.per_category.keys())
            .collect();
        for key in all_keys {
            let before_n = v.before.per_category.get(key).copied().unwrap_or(0);
            let after_n = v.after.per_category.get(key).copied().unwrap_or(0);
            if before_n != after_n {
                let (sign, magnitude) = if after_n >= before_n {
                    ('+', after_n - before_n)
                } else {
                    ('-', before_n - after_n)
                };
                writeln!(out, "  {key}: {before_n} → {after_n} ({sign}{magnitude})")?;
            }
        }
        writeln!(out, "{divider}")?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::{ScrubArgs, handle, wants_detector};

    #[test]
    fn wants_all_detectors_when_list_is_empty() {
        assert!(wants_detector(&[], "lexical"));
        assert!(wants_detector(&[], "comments"));
        assert!(wants_detector(&[], "readme"));
    }

    #[test]
    fn wants_only_named_detector_when_list_is_set() {
        let detectors = vec!["lexical".to_owned()];
        assert!(wants_detector(&detectors, "lexical"));
        assert!(!wants_detector(&detectors, "comments"));
    }

    #[test]
    fn dry_run_does_not_modify_files() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = TempDir::new()?;
        let path = tmp.path().join("slop.rs");
        let original = "// robust seamless delve facilitate comprehensive utilize
fn main() {}
";
        fs::write(&path, original)?;

        handle(&ScrubArgs {
            path: tmp.path().to_string_lossy().into_owned(),
            dry_run: true,
            detectors: vec![],
            ci: false,
            allow_unsafe_scrub: false,
            verify: false,
        })?;

        assert_eq!(fs::read_to_string(&path)?, original);
        Ok(())
    }

    #[test]
    fn live_run_replaces_slop_vocabulary() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = TempDir::new()?;
        let path = tmp.path().join("slop.rs");
        let original = "// robust seamless delve facilitate utilize\nfn main() {}\n";
        fs::write(&path, original)?;

        handle(&ScrubArgs {
            path: tmp.path().to_string_lossy().into_owned(),
            dry_run: false,
            detectors: vec![],
            ci: false,
            allow_unsafe_scrub: false,
            verify: false,
        })?;

        let after = fs::read_to_string(&path)?;
        assert_ne!(after, original);
        assert!(!after.contains("robust"));
        Ok(())
    }

    #[test]
    fn handle_empty_dir_returns_ok() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = TempDir::new()?;
        handle(&ScrubArgs {
            path: tmp.path().to_string_lossy().into_owned(),
            dry_run: false,
            detectors: vec![],
            ci: false,
            allow_unsafe_scrub: false,
            verify: false,
        })?;
        Ok(())
    }
}
