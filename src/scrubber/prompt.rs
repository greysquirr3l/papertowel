use std::fs;
use std::path::{Path, PathBuf};

use crate::detection::finding::{Finding, FindingCategory, LineRange, Severity};
use crate::domain::errors::PapertowelError;

pub const DETECTOR_NAME: &str = "prompt";

const PROMPT_LEAKAGE_MARKERS: [&str; 10] = [
    "as an ai language model",
    "i can't assist with",
    "i cannot help with",
    "let's break this down",
    "here's the updated",
    "assistant:",
    "user:",
    "analysis:",
    "chain of thought",
    "prompt:",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromptDetectionConfig {
    pub min_marker_hits: usize,
}

impl Default for PromptDetectionConfig {
    fn default() -> Self {
        Self { min_marker_hits: 2 }
    }
}

pub fn detect_file(path: impl AsRef<Path>) -> Result<Vec<Finding>, PapertowelError> {
    let path = path.as_ref();
    let content =
        fs::read_to_string(path).map_err(|error| PapertowelError::io_with_path(path, error))?;
    detect_in_text(path, &content, PromptDetectionConfig::default())
}

#[expect(
    clippy::cast_precision_loss,
    reason = "confidence score: bounded usize counts"
)]
pub fn detect_in_text(
    file_path: impl Into<PathBuf>,
    content: &str,
    config: PromptDetectionConfig,
) -> Result<Vec<Finding>, PapertowelError> {
    let file_path = file_path.into();
    let lowered = content.to_ascii_lowercase();

    let hits = PROMPT_LEAKAGE_MARKERS
        .iter()
        .filter(|marker| lowered.contains(**marker))
        .count();

    if hits < config.min_marker_hits {
        return Ok(Vec::new());
    }

    let severity = if hits >= 4 {
        Severity::High
    } else {
        Severity::Medium
    };
    let confidence = (hits as f32 / 8.0).min(1.0);
    let lines = content.lines().count().max(1);

    let mut finding = Finding::new(
        "prompt.leakage.markers",
        FindingCategory::PromptLeakage,
        severity,
        confidence,
        file_path,
        format!("Detected probable prompt leakage markers ({hits} matched phrases)."),
    )?;
    finding.line_range = Some(LineRange::new(1, lines)?);
    finding.suggestion = Some(
		"Remove chat transcript residue and rewrite text to repository-specific engineering context."
			.to_owned(),
	);

    Ok(vec![finding])
}

#[cfg(test)]
mod tests {
    use crate::scrubber::prompt::{DETECTOR_NAME, PromptDetectionConfig, detect_in_text};

    #[test]
    fn detector_name_is_stable() {
        assert_eq!(DETECTOR_NAME, "prompt");
    }

    #[test]
    fn prompt_detector_ignores_clean_text() -> Result<(), Box<dyn std::error::Error>> {
        let content = "This repository implements a lexical detector for source files.";
        let findings = detect_in_text("README.md", content, PromptDetectionConfig::default())?;
        assert!(findings.is_empty());
        Ok(())
    }

    #[test]
    fn prompt_detector_flags_leakage_markers() -> Result<(), Box<dyn std::error::Error>> {
        let content =
            "As an AI language model, let's break this down. Assistant: here's the updated patch.";
        let findings = detect_in_text("README.md", content, PromptDetectionConfig::default())?;
        assert_eq!(findings.len(), 1);
        Ok(())
    }
}
