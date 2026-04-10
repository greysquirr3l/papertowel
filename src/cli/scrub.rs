use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use clap::Args;
use walkdir::WalkDir;

use crate::config::{is_ignored, resolve_config};
use crate::detection::language::LanguageKind;
use crate::recipe::loader::RecipeLoader;
use crate::recipe::scrubber::RecipeScrubber;
use crate::scrubber::ignore_directives;
use crate::scrubber::{comments, readme};

pub const RECIPE_DETECTOR_NAME: &str = "recipe";

#[derive(Debug, Args)]
pub struct ScrubArgs {
    pub path: String,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long, value_delimiter = ',')]
    pub detectors: Vec<String>,
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
    let (project_root, _config, ignore) = resolve_config(&root)?;

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

    for path in &files {
        // Respect inline ignore-file directives.
        let skip = ignore_directives::parse_file(path)
            .map(|d| d.ignore_file)
            .unwrap_or(false);
        if skip {
            continue;
        }

        let result = apply_transforms(path, args, recipe_scrubber.as_deref());
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
        }
    }

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    write_report(&mut out, &changed_results, &summary, args.dry_run)?;

    Ok(())
}

fn apply_transforms(
    path: &Path,
    args: &ScrubArgs,
    recipe_scrubber: Option<&RecipeScrubber>,
) -> FileResult {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    let lang = LanguageKind::from_extension(ext);

    let mut result = FileResult {
        path: path.to_path_buf(),
        recipe: None,
        comments: None,
        readme: None,
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

    result
}

fn write_report(
    out: &mut impl Write,
    results: &[FileResult],
    summary: &ScrubSummary,
    dry_run: bool,
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
        })?;
        Ok(())
    }
}
