use std::path::Path;

use tracing::debug;
use walkdir::WalkDir;

use crate::config::{build_ignore_matcher, is_ignored, load_config};
use crate::detection::language::LanguageKind;
use crate::domain::errors::PapertowelError;
use crate::scrubber::comments::analyze_comments;
use crate::scrubber::lexical::SLOP_PATTERNS;

use super::baseline::{now_unix_secs, StyleBaseline};

/// Analyse all source files under `root` and derive a [`StyleBaseline`].
///
/// Files matching `.papertowelignore` and the repo's config exclusions are
/// skipped.  Files shorter than 8 non-empty lines are skipped to avoid
/// skewing averages with trivial files.
#[expect(
    clippy::cast_precision_loss,
    reason = "usize line/file counts: no meaningful precision loss at these scales"
)]
pub fn extract_baseline(root: &Path) -> Result<StyleBaseline, PapertowelError> {
    let config = load_config(root).unwrap_or_default();
    let ignore = build_ignore_matcher(root, &config)?;

    let mut total_comment_density = 0.0_f32;
    let mut total_doc_ratio = 0.0_f32;
    let mut total_slop_hits: usize = 0;
    let mut total_lines: usize = 0;
    let mut files_counted: usize = 0;

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            !ignore
                .as_ref()
                .is_some_and(|m| is_ignored(m, root, e.path(), false))
        })
    {
        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        let lang = LanguageKind::from_extension(ext);
        if !lang.is_analysable() {
            continue;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                debug!(path = %path.display(), error = %e, "skipping unreadable file");
                continue;
            }
        };

        let metrics = analyze_comments(&content);
        if metrics.non_empty_lines < 8 {
            continue;
        }

        total_comment_density += metrics.density;
        total_doc_ratio += if metrics.comment_lines > 0 {
            // doc ratio: proportion of comment lines starting with `///` or `//!`
            let doc_lines = content
                .lines()
                .filter(|l| {
                    let t = l.trim_start();
                    t.starts_with("///") || t.starts_with("//!")
                })
                .count();
            doc_lines as f32 / metrics.comment_lines as f32
        } else {
            0.0
        };

        let slop_hits = count_slop_hits(&content);
        total_slop_hits += slop_hits;
        total_lines += metrics.non_empty_lines;
        files_counted += 1;
    }

    if files_counted == 0 {
        return Err(PapertowelError::Validation(
            "no analysable source files found under the given path".to_owned(),
        ));
    }

    let avg_comment_density = total_comment_density / files_counted as f32;
    let avg_doc_ratio = total_doc_ratio / files_counted as f32;
    let slop_rate_per_hundred = if total_lines > 0 {
        (total_slop_hits as f32 / total_lines as f32) * 100.0
    } else {
        0.0
    };

    Ok(StyleBaseline {
        avg_comment_density,
        avg_doc_ratio,
        slop_rate_per_hundred,
        files_analyzed: files_counted,
        lines_analyzed: total_lines,
        created_at: now_unix_secs(),
    })
}

/// Count occurrences of slop vocabulary words in `content`.
fn count_slop_hits(content: &str) -> usize {
    let lower = content.to_lowercase();
    SLOP_PATTERNS
        .iter()
        .map(|pattern| {
            let needle = pattern.to_lowercase();
            let mut count = 0;
            let mut start = 0;
            while let Some(pos) = lower[start..].find(needle.as_str()) {
                count += 1;
                start += pos + needle.len();
            }
            count
        })
        .sum()
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::extract_baseline;

    fn make_repo(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().expect("tempdir");
        for (name, content) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("mkdir");
            }
            fs::write(path, content).expect("write");
        }
        dir
    }

    #[test]
    fn baseline_reflects_comment_density() {
        let src = "fn main() {\n// comment\n// comment\n// comment\nlet x = 1;\nlet y = 2;\nlet z = 3;\nlet w = 4;\nlet v = 5;\nlet u = x + y + z + w + v;\nprintln!(\"{u}\");\n}\n";
        let dir = make_repo(&[("src/main.rs", src)]);
        let baseline = extract_baseline(dir.path()).expect("baseline");
        assert!(baseline.files_analyzed >= 1);
        assert!(baseline.avg_comment_density > 0.0);
    }

    #[test]
    fn error_on_empty_dir() {
        let dir = TempDir::new().expect("tempdir");
        let result = extract_baseline(dir.path());
        assert!(result.is_err(), "should error with no analysable files");
    }
}
