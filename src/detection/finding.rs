use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::domain::errors::PapertowelError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FindingCategory {
    Lexical,
    Comment,
    Structure,
    Readme,
    Metadata,
    Workflow,
    Maintenance,
    Promotion,
    NameCredibility,
    IdiomMismatch,
    TestPattern,
    PromptLeakage,
    CommitPattern,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineRange {
    pub start: usize,
    pub end: usize,
}

impl LineRange {
    pub fn new(start: usize, end: usize) -> Result<Self, PapertowelError> {
        if start == 0 || end == 0 {
            return Err(PapertowelError::Validation(
                "line range must be 1-based".to_owned(),
            ));
        }

        if start > end {
            return Err(PapertowelError::Validation(
                "line range start must be <= end".to_owned(),
            ));
        }

        Ok(Self { start, end })
    }

    pub fn contains(self, line: usize) -> bool {
        line >= self.start && line <= self.end
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub category: FindingCategory,
    pub severity: Severity,
    pub confidence_score: f32,
    pub file_path: PathBuf,
    pub line_range: Option<LineRange>,
    pub description: String,
    pub suggestion: Option<String>,
    pub auto_fixable: bool,
}

impl Finding {
    pub fn new(
        id: impl Into<String>,
        category: FindingCategory,
        severity: Severity,
        confidence_score: f32,
        file_path: impl Into<PathBuf>,
        description: impl Into<String>,
    ) -> Result<Self, PapertowelError> {
        let id = id.into();
        if id.trim().is_empty() {
            return Err(PapertowelError::Validation(
                "finding id must not be empty".to_owned(),
            ));
        }

        if !(0.0..=1.0).contains(&confidence_score) {
            return Err(PapertowelError::Validation(
                "confidence score must be within 0.0..=1.0".to_owned(),
            ));
        }

        let description = description.into();
        if description.trim().is_empty() {
            return Err(PapertowelError::Validation(
                "finding description must not be empty".to_owned(),
            ));
        }

        Ok(Self {
            id,
            category,
            severity,
            confidence_score,
            file_path: file_path.into(),
            line_range: None,
            description,
            suggestion: None,
            auto_fixable: false,
        })
    }

    pub fn is_high_confidence(&self) -> bool {
        self.confidence_score >= 0.75
    }
}

#[cfg(test)]
mod tests {
    use super::{Finding, FindingCategory, LineRange, Severity};

    #[test]
    fn line_range_rejects_invalid_bounds() {
        assert!(LineRange::new(0, 10).is_err());
        assert!(LineRange::new(12, 10).is_err());
    }

    #[test]
    fn line_range_contains_expected_lines() {
        let range_result = LineRange::new(10, 12);
        assert!(range_result.is_ok());
        let range = match range_result {
            Ok(range) => range,
            Err(error) => panic!("unexpected line range error: {error}"),
        };
        assert!(range.contains(10));
        assert!(range.contains(12));
        assert!(!range.contains(13));
    }

    #[test]
    fn finding_constructor_validates_fields() {
        let finding_result = Finding::new(
            "lexical.cluster",
            FindingCategory::Lexical,
            Severity::High,
            0.9,
            "src/lib.rs",
            "High-density slop phrase cluster detected",
        );
        assert!(finding_result.is_ok());
        let finding = match finding_result {
            Ok(finding) => finding,
            Err(error) => panic!("unexpected finding error: {error}"),
        };

        assert!(finding.is_high_confidence());
        assert_eq!(finding.category, FindingCategory::Lexical);
    }

    #[test]
    fn finding_constructor_rejects_invalid_confidence() {
        let finding = Finding::new(
            "lexical.cluster",
            FindingCategory::Lexical,
            Severity::Low,
            1.2,
            "src/lib.rs",
            "invalid confidence",
        );

        assert!(finding.is_err());
    }
}