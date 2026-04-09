use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Args;
use walkdir::WalkDir;

use crate::config::{build_ignore_matcher, is_ignored, load_config};
use crate::detection::language::LanguageKind;
use crate::scrubber::{comments, lexical, readme};

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
 lexical_replacements: usize,
 comment_lines_removed: usize,
 readme_lines_removed: usize,
}

struct FileResult {
 path: PathBuf,
 /// Lexical replacements applied (or that would be applied).
 lexical: Option<usize>,
 /// Over-documentation comment lines removed.
 comments: Option<usize>,
 /// README scaffold lines removed.
 readme: Option<usize>,
}

impl FileResult {
 const fn changed(&self) -> bool {
 self.lexical.is_some() || self.comments.is_some() || self.readme.is_some()
 }
}

fn wants_detector(detectors: &[String], name: &str) -> bool {
 detectors.is_empty() || detectors.iter().any(|d| d == name)
}

pub fn handle(args: &ScrubArgs) -> Result<()> {
 let root = PathBuf::from(&args.path);
 let config = load_config(&root).unwrap_or_default();
 let ignore = build_ignore_matcher(&root, &config)?;

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

 let mut changed_results: Vec<FileResult> = Vec::new();
 let mut summary = ScrubSummary::default();

 for path in &files {
 let result = apply_transforms(path, args);
 if result.changed() {
 summary.files_changed += 1;
 if let Some(n) = result.lexical {
 summary.lexical_replacements += n;
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

fn apply_transforms(path: &Path, args: &ScrubArgs) -> FileResult {
 let ext = path
.extension()
.and_then(|e| e.to_str())
.unwrap_or_default();
 let lang = LanguageKind::from_extension(ext);

 let mut result = FileResult {
 path: path.to_path_buf(),
 lexical: None,
 comments: None,
 readme: None,
 };

 if lang.is_analysable() {
 if wants_detector(&args.detectors, lexical::DETECTOR_NAME) {
 match lexical::transform_file(path, args.dry_run) {
 Ok(r) if r.changed => result.lexical = Some(r.replacements_applied),
 Ok(_) => {}
 Err(e) => tracing::warn!(path = %path.display(), "lexical transform error: {e}"),
 }
 }
 if wants_detector(&args.detectors, comments::DETECTOR_NAME) {
 match comments::transform_file(path, args.dry_run) {
 Ok(r) if r.changed => result.comments = Some(r.removed_comment_lines),
 Ok(_) => {}
 Err(e) => tracing::warn!(path = %path.display(), "comments transform error: {e}"),
 }
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
 if let Some(n) = r.lexical {
 writeln!(
 out,
 " [lexical] {n} replacement{}",
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
 if summary.lexical_replacements > 0 {
 write!(out, " · {} lexical", summary.lexical_replacements)?;
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
 let original = "// solid detailed use use help\nfn main() {}\n";
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
 let original = "// solid detailed use use help smooth scaffold\nfn main() {}\n";
 fs::write(&path, original)?;

 handle(&ScrubArgs {
 path: tmp.path().to_string_lossy().into_owned(),
 dry_run: false,
 detectors: vec!["lexical".to_owned()],
 })?;

 let after = fs::read_to_string(&path)?;
 assert_ne!(after, original);
 assert!(!after.contains("solid"));
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