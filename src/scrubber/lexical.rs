use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use aho_corasick::AhoCorasick;

use crate::detection::finding::{Finding, FindingCategory, LineRange, Severity};
use crate::domain::errors::PapertowelError;

pub const DETECTOR_NAME: &str = "lexical";

const SLOP_PATTERNS: [&str; 32] = [
    "robust",
    "comprehensive",
    "streamlined",
    "utilize",
    "facilitate",
    "leverage",
    "seamless",
    "delve",
    "extensible",
    "boilerplate",
    "granular",
    "opinionated",
    "it's worth noting",
    "as mentioned above",
    "for the sake of",
    "in order to",
    "this ensures that",
    "helper function to",
    "this module provides",
    "we can see that",
    "under the hood",
    "out of the box",
    "at the end of the day",
    "easy-to-use",
    "production-ready",
    "enterprise-grade",
    "clean and intuitive",
    "modern and scalable",
    "best-in-class",
    "next-generation",
    "state-of-the-art",
    "synergy",
];

static MATCHER: LazyLock<AhoCorasick> = LazyLock::new(|| {
    let built = AhoCorasick::builder()
        .ascii_case_insensitive(true)
        .build(SLOP_PATTERNS);

    match built {
        Ok(matcher) => matcher,
        Err(error) => panic!("failed to build lexical matcher: {error}"),
    }
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LexicalDetectionConfig {
    pub min_matches: usize,
    pub min_unique_terms: usize,
    pub high_severity_match_count: usize,
}

impl Default for LexicalDetectionConfig {
    fn default() -> Self {
        Self {
            min_matches: 4,
            min_unique_terms: 3,
            high_severity_match_count: 8,
        }
    }
}

#[must_use]
pub fn corpus() -> &'static [&'static str] {
    &SLOP_PATTERNS
}

pub fn detect_file(path: impl AsRef<Path>) -> Result<Vec<Finding>, PapertowelError> {
    let path = path.as_ref();
    let content =
        fs::read_to_string(path).map_err(|error| PapertowelError::io_with_path(path, error))?;
    detect_in_text(path, &content, LexicalDetectionConfig::default())
}

pub fn detect_in_text(
    file_path: impl Into<PathBuf>,
    content: &str,
    config: LexicalDetectionConfig,
) -> Result<Vec<Finding>, PapertowelError> {
    let file_path = file_path.into();

    let mut total_matches = 0_usize;
    let mut terms = BTreeSet::new();
    let mut first_offset = None;
    let mut last_offset = None;

    for candidate in MATCHER.find_iter(content) {
        total_matches += 1;

        if first_offset.is_none() {
            first_offset = Some(candidate.start());
        }
        last_offset = Some(candidate.end());

        let index = candidate.pattern().as_usize();
        if let Some(term) = SLOP_PATTERNS.get(index) {
            terms.insert((*term).to_owned());
        }
    }

    if total_matches < config.min_matches || terms.len() < config.min_unique_terms {
        return Ok(Vec::new());
    }

    let severity = if total_matches >= config.high_severity_match_count {
        Severity::High
    } else {
        Severity::Medium
    };

    let confidence_score = ((total_matches as f32 / 12.0) + (terms.len() as f32 / 12.0)).min(1.0);

    let line_range = build_line_range(content, first_offset, last_offset)?;
    let sample_terms = terms.iter().take(4).cloned().collect::<Vec<_>>().join(", ");
    let description = format!(
        "Detected lexical slop cluster ({} matches, {} unique terms): {}",
        total_matches,
        terms.len(),
        sample_terms
    );

    let mut finding = Finding::new(
        "lexical.cluster",
        FindingCategory::Lexical,
        severity,
        confidence_score,
        file_path,
        description,
    )?;
    finding.line_range = line_range;
    finding.suggestion = Some(
        "Replace repeated assistant-style vocabulary with concise, repository-specific language."
            .to_owned(),
    );
    finding.auto_fixable = false;

    Ok(vec![finding])
}

fn build_line_range(
    content: &str,
    first_offset: Option<usize>,
    last_offset: Option<usize>,
) -> Result<Option<LineRange>, PapertowelError> {
    match (first_offset, last_offset) {
        (Some(start), Some(end)) => {
            let start_line = line_number_at_offset(content, start);
            let end_line = line_number_at_offset(content, end);
            LineRange::new(start_line, end_line).map(Some)
        }
        _ => Ok(None),
    }
}

fn line_number_at_offset(content: &str, offset: usize) -> usize {
    content
        .char_indices()
        .take_while(|(index, _)| *index < offset)
        .fold(
            1_usize,
            |line, (_, ch)| if ch == '\n' { line + 1 } else { line },
        )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use crate::detection::finding::Severity;
    use crate::scrubber::lexical::{
        DETECTOR_NAME, LexicalDetectionConfig, corpus, detect_file, detect_in_text,
    };

    #[test]
    fn detector_name_is_stable() {
        assert_eq!(DETECTOR_NAME, "lexical");
    }

    #[test]
    fn corpus_contains_key_reference_phrase() {
        assert!(corpus().iter().any(|term| *term == "it's worth noting"));
    }

    #[test]
    fn detect_in_text_returns_empty_for_sparse_terms() {
        let findings = detect_in_text(
            "src/lib.rs",
            "This module is robust in exactly one spot.",
            LexicalDetectionConfig::default(),
        );

        assert!(findings.is_ok());
        let findings = match findings {
            Ok(findings) => findings,
            Err(error) => panic!("unexpected detector error: {error}"),
        };
        assert!(findings.is_empty());
    }

    #[test]
    fn detect_in_text_flags_clustered_slop() {
        let sample = "\
			This module provides a comprehensive and robust approach.\n\
			It's worth noting that we can see that the design is streamlined.\n\
			In order to facilitate a seamless experience, this ensures that things work out of the box.\n\
		";

        let findings = detect_in_text("src/lib.rs", sample, LexicalDetectionConfig::default());
        assert!(findings.is_ok());
        let findings = match findings {
            Ok(findings) => findings,
            Err(error) => panic!("unexpected detector error: {error}"),
        };

        assert_eq!(findings.len(), 1);
        let first = findings.first();
        assert!(first.is_some());
        let first = match first {
            Some(first) => first,
            None => panic!("expected first finding"),
        };
        assert!(matches!(first.severity, Severity::Medium | Severity::High));
        assert!(first.line_range.is_some());
    }

    #[test]
    fn detect_file_reads_and_processes_content() {
        let tmp = TempDir::new();
        assert!(tmp.is_ok());
        let tmp = match tmp {
            Ok(tmp) => tmp,
            Err(error) => panic!("failed to create tempdir: {error}"),
        };
        let file_path = tmp.path().join("sample.rs");

        let write_result = fs::write(
            &file_path,
            "this module provides a comprehensive approach that is streamlined and robust",
        );
        assert!(write_result.is_ok());

        let findings = detect_file(&file_path);
        assert!(findings.is_ok());
        let findings = match findings {
            Ok(findings) => findings,
            Err(error) => panic!("unexpected detector error: {error}"),
        };
        assert_eq!(findings.len(), 1);
    }
}
