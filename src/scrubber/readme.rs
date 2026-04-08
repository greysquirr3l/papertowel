use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::detection::finding::{Finding, FindingCategory, LineRange, Severity};
use crate::domain::errors::PapertowelError;

pub const DETECTOR_NAME: &str = "readme";

const TEMPLATE_SECTION_HEADINGS: [&str; 9] = [
    "installation",
    "usage",
    "features",
    "roadmap",
    "contributing",
    "license",
    "acknowledgements",
    "faq",
    "getting started",
];

const TEMPLATE_PHRASES: [&str; 8] = [
    "replace this section",
    "add your project description",
    "feel free to",
    "open an issue",
    "pull requests are welcome",
    "this project was bootstrapped",
    "made with",
    "template",
];

const SECTION_DROP_CANDIDATES: [&str; 6] = [
    "getting started",
    "roadmap",
    "acknowledgements",
    "faq",
    "features",
    "installation",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadmeDetectionConfig {
    pub min_template_sections: usize,
    pub min_template_phrases: usize,
}

impl Default for ReadmeDetectionConfig {
    fn default() -> Self {
        Self {
            min_template_sections: 5,
            min_template_phrases: 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadmeTransformResult {
    pub removed_lines: usize,
    pub changed: bool,
}

pub fn detect_file(path: impl AsRef<Path>) -> Result<Vec<Finding>, PapertowelError> {
    let path = path.as_ref();
    let content =
        fs::read_to_string(path).map_err(|error| PapertowelError::io_with_path(path, error))?;
    detect_in_text(path, &content, ReadmeDetectionConfig::default())
}

pub fn detect_in_text(
    file_path: impl Into<PathBuf>,
    content: &str,
    config: ReadmeDetectionConfig,
) -> Result<Vec<Finding>, PapertowelError> {
    let file_path = file_path.into();
    let lowered = content.to_ascii_lowercase();

    let heading_terms = collect_heading_terms(content);
    let section_hits = TEMPLATE_SECTION_HEADINGS
        .iter()
        .filter(|section| heading_terms.contains(**section))
        .count();
    let phrase_hits = TEMPLATE_PHRASES
        .iter()
        .filter(|phrase| lowered.contains(**phrase))
        .count();

    if section_hits < config.min_template_sections || phrase_hits < config.min_template_phrases {
        return Ok(Vec::new());
    }

    let severity = if section_hits >= 7 && phrase_hits >= 3 {
        Severity::High
    } else {
        Severity::Medium
    };
    let confidence =
        ((section_hits as f32 / 9.0) * 0.7 + (phrase_hits as f32 / 6.0) * 0.3).min(1.0);

    let end_line = content.lines().count().max(1);
    let mut finding = Finding::new(
        "readme.template.structure",
        FindingCategory::Readme,
        severity,
        confidence,
        file_path,
        format!(
            "README appears template-heavy ({} boilerplate sections, {} template phrases).",
            section_hits, phrase_hits
        ),
    )?;
    finding.line_range = Some(LineRange::new(1, end_line)?);
    finding.suggestion = Some(
		"Replace generic sections with project-specific engineering details, examples, and real maintenance guidance."
			.to_owned(),
	);

    Ok(vec![finding])
}

pub fn transform_file(path: impl AsRef<Path>, dry_run: bool) -> Result<ReadmeTransformResult, PapertowelError> {
    let path = path.as_ref();
    let original = fs::read_to_string(path).map_err(|error| PapertowelError::io_with_path(path, error))?;
    let (transformed, result) = transform_text(&original);

    if !dry_run && result.changed {
        fs::write(path, transformed).map_err(|error| PapertowelError::io_with_path(path, error))?;
    }

    Ok(result)
}

#[must_use]
pub fn transform_text(content: &str) -> (String, ReadmeTransformResult) {
    let lines = content.lines().map(ToOwned::to_owned).collect::<Vec<_>>();
    if lines.is_empty() {
        return (
            String::new(),
            ReadmeTransformResult {
                removed_lines: 0,
                changed: false,
            },
        );
    }

    let heading_indices = lines
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                Some(idx)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    let mut drop_indices = BTreeSet::new();
    for (idx, line) in lines.iter().enumerate() {
        let lowered = line.to_ascii_lowercase();
        if TEMPLATE_PHRASES
            .iter()
            .any(|phrase| lowered.contains(phrase))
        {
            drop_indices.insert(idx);
        }
    }

    for (position, heading_idx) in heading_indices.iter().enumerate() {
        let heading_line = lines.get(*heading_idx).map(|line| line.trim()).unwrap_or("");
        let normalized_heading = heading_line
            .trim_start_matches('#')
            .trim()
            .to_ascii_lowercase();

        let next_heading = heading_indices.get(position + 1).copied().unwrap_or(lines.len());
        let content_line_count = section_content_line_count(
            &lines,
            heading_idx + 1,
            next_heading,
            &drop_indices,
        );

        if SECTION_DROP_CANDIDATES.contains(&normalized_heading.as_str()) && content_line_count == 0 {
            drop_indices.insert(*heading_idx);
        }
    }

    let mut output = Vec::new();
    let mut removed_lines = 0_usize;
    let mut last_blank = false;
    for (idx, line) in lines.iter().enumerate() {
        if drop_indices.contains(&idx) {
            removed_lines += 1;
            continue;
        }

        let is_blank = line.trim().is_empty();
        if is_blank && last_blank {
            continue;
        }

        output.push(line.clone());
        last_blank = is_blank;
    }

    let mut transformed = output.join("\n");
    if content.ends_with('\n') {
        transformed.push('\n');
    }

    let changed = transformed != content;
    (
        transformed,
        ReadmeTransformResult {
            removed_lines,
            changed,
        },
    )
}

fn collect_heading_terms(content: &str) -> BTreeSet<String> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if !trimmed.starts_with('#') {
                return None;
            }
            let heading = trimmed.trim_start_matches('#').trim().to_ascii_lowercase();
            if heading.is_empty() {
                None
            } else {
                Some(heading)
            }
        })
        .collect()
}

fn section_content_line_count(
    lines: &[String],
    start: usize,
    end: usize,
    drop_indices: &BTreeSet<usize>,
) -> usize {
    lines
        .iter()
        .enumerate()
        .skip(start)
        .take(end.saturating_sub(start))
        .filter(|(idx, line)| !drop_indices.contains(idx) && !line.trim().is_empty())
        .count()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use crate::scrubber::readme::{
        DETECTOR_NAME, ReadmeDetectionConfig, detect_in_text, transform_file, transform_text,
    };

    #[test]
    fn detector_name_is_stable() {
        assert_eq!(DETECTOR_NAME, "readme");
    }

    #[test]
    fn readme_detector_ignores_repo_specific_content() {
        let content =
            "# papertowel\n\n## Architecture\nReal details here.\n## Commands\nActual examples.\n";
        let findings = detect_in_text("README.md", content, ReadmeDetectionConfig::default());
        assert!(findings.is_ok());
        let findings = match findings {
            Ok(findings) => findings,
            Err(error) => panic!("unexpected readme detector error: {error}"),
        };
        assert!(findings.is_empty());
    }

    #[test]
    fn readme_detector_flags_template_bundles() {
        let content = "\
# My Project\n\
## Installation\n\
## Usage\n\
## Features\n\
## Roadmap\n\
## Contributing\n\
## License\n\
## Acknowledgements\n\
Feel free to open an issue.\n\
Pull requests are welcome.\n\
This project was bootstrapped from a template.\n\
";

        let findings = detect_in_text("README.md", content, ReadmeDetectionConfig::default());
        assert!(findings.is_ok());
        let findings = match findings {
            Ok(findings) => findings,
            Err(error) => panic!("unexpected readme detector error: {error}"),
        };
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn transform_text_removes_template_phrases_and_empty_sections() {
        let content = "\
# My Project\n\
## Installation\n\
\n\
## Usage\n\
Run with cargo run -- --help\n\
## Acknowledgements\n\
\n\
This project was bootstrapped from a template.\n";

        let (transformed, result) = transform_text(content);

        assert!(result.changed);
        assert!(result.removed_lines >= 2);
        assert!(!transformed.contains("bootstrapped"));
        assert!(!transformed.contains("## Acknowledgements"));
        assert!(transformed.contains("## Usage"));
    }

    #[test]
    fn transform_file_honors_dry_run() {
        let tmp = TempDir::new();
        assert!(tmp.is_ok());
        let tmp = match tmp {
            Ok(tmp) => tmp,
            Err(error) => panic!("failed to create tempdir: {error}"),
        };
        let file_path = tmp.path().join("README.md");

        let write_result = fs::write(&file_path, "# Demo\nThis project was bootstrapped from a template.\n");
        assert!(write_result.is_ok());

        let result = transform_file(&file_path, true);
        assert!(result.is_ok());
        let result = match result {
            Ok(result) => result,
            Err(error) => panic!("unexpected transform error: {error}"),
        };
        assert!(result.changed);

        let disk_content = fs::read_to_string(&file_path);
        assert!(disk_content.is_ok());
        let disk_content = match disk_content {
            Ok(content) => content,
            Err(error) => panic!("unexpected read error: {error}"),
        };
        assert!(disk_content.contains("bootstrapped"));
    }
}
