use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::detection::finding::{Finding, FindingCategory, LineRange, Severity};
use crate::domain::errors::PapertowelError;

pub const DETECTOR_NAME: &str = "comments";

const TUTORIAL_PHRASES: [&str; 8] = [
    "this function",
    "this module",
    "helper to",
    "we can see",
    "as mentioned",
    "in order to",
    "this ensures",
    "for the sake of",
];

const PRESERVE_COMMENT_HINTS: [&str; 12] = [
    "safety",
    "invariant",
    "why",
    "because",
    "security",
    "caveat",
    "trade-off",
    "todo",
    "fixme",
    "hack",
    "xxx",
    "note:",
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CommentDetectionConfig {
    pub min_non_empty_lines: usize,
    pub high_density_threshold: f32,
    pub tutorial_phrase_threshold: usize,
    pub uniform_prefix_threshold: f32,
}

impl Default for CommentDetectionConfig {
    fn default() -> Self {
        Self {
            min_non_empty_lines: 12,
            high_density_threshold: 0.45,
            tutorial_phrase_threshold: 3,
            uniform_prefix_threshold: 0.65,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CommentMetrics {
    pub non_empty_lines: usize,
    pub comment_lines: usize,
    pub density: f32,
    pub tutorial_phrase_hits: usize,
    pub dominant_prefix_ratio: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommentTransformResult {
    pub removed_comment_lines: usize,
    pub changed: bool,
}

pub fn detect_file(path: impl AsRef<Path>) -> Result<Vec<Finding>, PapertowelError> {
    let path = path.as_ref();
    let content =
        fs::read_to_string(path).map_err(|error| PapertowelError::io_with_path(path, error))?;
    detect_in_text(path, &content, CommentDetectionConfig::default())
}

#[expect(clippy::cast_precision_loss, reason = "confidence score: bounded usize counts")]
pub fn detect_in_text(
    file_path: impl Into<PathBuf>,
    content: &str,
    config: CommentDetectionConfig,
) -> Result<Vec<Finding>, PapertowelError> {
    let file_path = file_path.into();
    let analysis = analyze_comments(content);

    if analysis.non_empty_lines < config.min_non_empty_lines {
        return Ok(Vec::new());
    }

    let over_dense = analysis.density >= config.high_density_threshold;
    let tutorial_heavy = analysis.tutorial_phrase_hits >= config.tutorial_phrase_threshold;
    let uniform = analysis.dominant_prefix_ratio >= config.uniform_prefix_threshold;

    if !(over_dense && (tutorial_heavy || uniform)) {
        return Ok(Vec::new());
    }

    let severity = if analysis.density > 0.60
        && analysis.tutorial_phrase_hits > config.tutorial_phrase_threshold
    {
        Severity::High
    } else {
        Severity::Medium
    };

    let confidence = analysis
        .density
        .mul_add(
            0.5,
            (analysis.tutorial_phrase_hits as f32 / 8.0)
                .mul_add(0.3, analysis.dominant_prefix_ratio * 0.2),
        )
        .min(1.0);

    let line_range = comment_line_range(content)?;
    let description = format!(
        "Detected over-documentation pattern: comment density {:.2}, tutorial-style hits {}, dominant prefix ratio {:.2}",
        analysis.density, analysis.tutorial_phrase_hits, analysis.dominant_prefix_ratio
    );

    let mut finding = Finding::new(
        "comments.over_documented",
        FindingCategory::Comment,
        severity,
        confidence,
        file_path,
        description,
    )?;
    finding.line_range = line_range;
    finding.suggestion = Some(
		"Keep comments for intent and safety context; remove repetitive narration of obvious code operations."
			.to_owned(),
	);

    Ok(vec![finding])
}

pub fn transform_file(path: impl AsRef<Path>, dry_run: bool) -> Result<CommentTransformResult, PapertowelError> {
    let path = path.as_ref();
    let original = fs::read_to_string(path).map_err(|error| PapertowelError::io_with_path(path, error))?;
    let (transformed, result) = transform_text(&original);

    if !dry_run && result.changed {
        fs::write(path, transformed).map_err(|error| PapertowelError::io_with_path(path, error))?;
    }

    Ok(result)
}

#[must_use]
pub fn transform_text(content: &str) -> (String, CommentTransformResult) {
    let mut output = Vec::new();
    let mut removed = 0_usize;
    let mut last_prefix: Option<String> = None;
    let mut last_output_blank = false;

    for raw_line in content.lines() {
        let trimmed = raw_line.trim();

        if !is_comment_line(trimmed) {
            output.push(raw_line.to_owned());
            last_prefix = None;
            last_output_blank = trimmed.is_empty();
            continue;
        }

        let body = normalize_comment_body(trimmed);
        let lowered = body.to_ascii_lowercase();
        let preserve = should_preserve_comment(&lowered);
        let tutorial = is_tutorial_comment(&lowered);
        let prefix = normalize_prefix(trimmed);

        let repeated_prefix = match (&last_prefix, &prefix) {
            (Some(previous), Some(current)) => previous == current,
            _ => false,
        };

        let drop_line = !preserve && (tutorial || repeated_prefix);
        if drop_line {
            removed += 1;
            continue;
        }

        if trimmed.is_empty() {
            if last_output_blank {
                continue;
            }
            last_output_blank = true;
        } else {
            last_output_blank = false;
        }

        output.push(raw_line.to_owned());
        last_prefix = prefix;
    }

    let mut transformed = output.join("\n");
    if content.ends_with('\n') {
        transformed.push('\n');
    }

    let changed = transformed != content;
    (
        transformed,
        CommentTransformResult {
            removed_comment_lines: removed,
            changed,
        },
    )
}

#[must_use]
#[expect(clippy::cast_precision_loss, reason = "density ratios: bounded usize counts")]
pub fn analyze_comments(content: &str) -> CommentMetrics {
    let mut non_empty_lines = 0_usize;
    let mut comment_lines = 0_usize;
    let mut tutorial_phrase_hits = 0_usize;
    let mut prefix_counts: HashMap<String, usize> = HashMap::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        non_empty_lines += 1;

        if !is_comment_line(trimmed) {
            continue;
        }

        comment_lines += 1;
        let lowered = trimmed.to_ascii_lowercase();

        if TUTORIAL_PHRASES
            .iter()
            .any(|phrase| lowered.contains(phrase))
        {
            tutorial_phrase_hits += 1;
        }

        if let Some(prefix) = normalize_prefix(trimmed) {
            let next_count = prefix_counts.get(&prefix).copied().unwrap_or(0_usize) + 1;
            prefix_counts.insert(prefix, next_count);
        }
    }

    let density = if non_empty_lines == 0 {
        0.0
    } else {
        comment_lines as f32 / non_empty_lines as f32
    };

    let dominant_prefix_ratio = if comment_lines == 0 {
        0.0
    } else {
        let max_prefix = prefix_counts.values().copied().max().unwrap_or(0_usize);
        max_prefix as f32 / comment_lines as f32
    };

    CommentMetrics {
        non_empty_lines,
        comment_lines,
        density,
        tutorial_phrase_hits,
        dominant_prefix_ratio,
    }
}

fn comment_line_range(content: &str) -> Result<Option<LineRange>, PapertowelError> {
    let mut first_line = None;
    let mut last_line = None;

    for (index, line) in content.lines().enumerate() {
        let line_number = index + 1;
        if is_comment_line(line.trim()) {
            if first_line.is_none() {
                first_line = Some(line_number);
            }
            last_line = Some(line_number);
        }
    }

    match (first_line, last_line) {
        (Some(start), Some(end)) => LineRange::new(start, end).map(Some),
        _ => Ok(None),
    }
}

fn is_comment_line(line: &str) -> bool {
    line.starts_with("//")
        || line.starts_with("///")
        || line.starts_with('#')
        || line.starts_with("/*")
        || line.starts_with('*')
}

fn normalize_comment_body(line: &str) -> String {
    line.trim_start_matches('/')
        .trim_start_matches('*')
        .trim_start_matches('#')
        .trim()
        .to_owned()
}

fn should_preserve_comment(lowered: &str) -> bool {
    PRESERVE_COMMENT_HINTS
        .iter()
        .any(|hint| lowered.contains(hint))
}

fn is_tutorial_comment(lowered: &str) -> bool {
    if TUTORIAL_PHRASES
        .iter()
        .any(|phrase| lowered.contains(phrase))
    {
        return true;
    }

    lowered.starts_with("returns ")
        || lowered.starts_with("sets ")
        || lowered.starts_with("initializes ")
        || lowered.starts_with("gets ")
}

fn normalize_prefix(line: &str) -> Option<String> {
    let comment = line
        .trim_start_matches('/')
        .trim_start_matches('*')
        .trim_start_matches('#')
        .trim();

    if comment.is_empty() {
        return None;
    }

    let lowered = comment.to_ascii_lowercase();
    let prefix = lowered
        .split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ");

    if prefix.is_empty() {
        None
    } else {
        Some(prefix)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use crate::detection::finding::Severity;
    use crate::scrubber::comments::{
        CommentDetectionConfig, DETECTOR_NAME, analyze_comments, detect_file, detect_in_text,
        transform_file, transform_text,
    };

    #[test]
    fn detector_name_is_stable() {
        assert_eq!(DETECTOR_NAME, "comments");
    }

    #[test]
    fn analyze_comments_reports_density() {
        let content = "// This function does x\nfn x() {}\n// helper to do y\n";
        let metrics = analyze_comments(content);
        assert_eq!(metrics.non_empty_lines, 3);
        assert_eq!(metrics.comment_lines, 2);
        assert!(metrics.density > 0.60);
    }

    #[test]
    fn detect_in_text_skips_light_commenting() -> Result<(), Box<dyn std::error::Error>> {
        let content = "fn one() {}\nfn two() {}\n// tiny note\nfn three() {}\n";
        let findings = detect_in_text("src/lib.rs", content, CommentDetectionConfig::default())?;
        assert!(findings.is_empty());
        Ok(())
    }

    #[test]
    fn detect_in_text_flags_tutorial_heavy_comments() -> Result<(), Box<dyn std::error::Error>> {
        let sample = "\
			// This module provides a robust interface\n\
			// This function computes a value in order to help users\n\
			// This function ensures that all states are valid\n\
			// This function returns the final result\n\
			fn run() {}\n\
			// This function logs telemetry\n\
			fn trace() {}\n\
		";

        let config = CommentDetectionConfig {
            min_non_empty_lines: 6,
            ..CommentDetectionConfig::default()
        };

        let findings = detect_in_text("src/lib.rs", sample, config)?;

        assert_eq!(findings.len(), 1);
        let Some(first) = findings.first() else {
            return Err("expected one finding".into());
        };
        assert!(matches!(first.severity, Severity::Medium | Severity::High));
        Ok(())
    }

    #[test]
    fn detect_file_processes_real_file() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = TempDir::new()?;
        let file_path = tmp.path().join("sample.rs");

        fs::write(
            &file_path,
            "// This function does a\n// This function does b\n// This function does c\n// This function does d\nfn run() {}\n",
        )?;

        let findings = detect_file(&file_path)?;
        assert_eq!(findings.len(), 0);
        Ok(())
    }

    #[test]
    fn transform_text_removes_tutorial_noise_and_keeps_safety_notes() {
        let sample = "\
// This function computes the value\n\
// This function returns the result\n\
// Safety: caller must hold the lock before invoking this path\n\
fn run() {}\n";

        let (transformed, result) = transform_text(sample);

        assert!(result.changed);
        assert!(result.removed_comment_lines >= 2);
        assert!(transformed.contains("Safety:"));
        assert!(!transformed.contains("This function computes"));
    }

    #[test]
    fn transform_file_honors_dry_run() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = TempDir::new()?;
        let file_path = tmp.path().join("sample.rs");

        fs::write(&file_path, "// This function does x\nfn x() {}\n")?;

        let result = transform_file(&file_path, true)?;
        assert!(result.changed);

        let disk_content = fs::read_to_string(&file_path)?;
        assert!(disk_content.contains("This function does x"));
        Ok(())
    }
}
