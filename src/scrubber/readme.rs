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

#[cfg(test)]
mod tests {
    use crate::scrubber::readme::{DETECTOR_NAME, ReadmeDetectionConfig, detect_in_text};

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
}
