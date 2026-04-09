use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use aho_corasick::AhoCorasick;
use regex::Regex;

use crate::detection::finding::{Finding, FindingCategory, LineRange, Severity};
use crate::domain::errors::PapertowelError;

pub const DETECTOR_NAME: &str = "lexical";

pub const SLOP_PATTERNS: [&str; 92] = [
    "accordingly",
    "additionally",
    "arguably",
    "certainly",
    "consequently",
    "hence",
    "however",
    "indeed",
    "moreover",
    "nevertheless",
    "nonetheless",
    "notwithstanding",
    "thus",
    "undoubtedly",
    "adept",
    "commendable",
    "ever-evolving",
    "exciting",
    "exemplary",
    "invaluable",
    "robust",
    "seamless",
    "synergistic",
    "thought-provoking",
    "transformative",
    "utmost",
    "vibrant",
    "vital",
    "innovative",
    "cutting-edge",
    "game-changing",
    "pivotal",
    "innovation",
    "tapestry",
    "realm",
    "landscape",
    "aligns",
    "augment",
    "delve",
    "embark",
    "facilitate",
    "leverage",
    "maximize",
    "underscores",
    "utilize",
    "harness",
    "illuminate",
    "revolutionize",
    "bolster",
    "streamline",
    "a testament to",
    "in summary",
    "in conclusion",
    "it\'s important to note",
    "it\'s important to consider",
    "it\'s worth noting that",
    "on the contrary",
    "that being said",
    "at its core",
    "to put it simply",
    "this underscores the importance of",
    "a key takeaway is",
    "from a broader perspective",
    "generally speaking",
    "broadly speaking",
    "tends to",
    "to some extent",
    "shed light on",
    "sheds light on",
    "seamless integration",
    "scalable solution",
    "actionable insights",
    "data-driven insights",
    "data-driven decisions",
    "leveraging",
    "this ensures that",
    "helper function to",
    "helper to",
    "this module provides",
    "this module offers",
    "we can see that",
    "under the hood",
    "out of the box",
    "at the end of the day",
    "as mentioned above",
    "for the sake of",
    "in order to",
    "a comprehensive",
    "provides a streamlined",
    "comprehensive",
    "ergonomic",
    "ready for production",
];

const SLOP_REPLACEMENTS: [&str; 92] = [
    "so",
    "also",
    "perhaps",
    "",
    "so",
    "so",
    "but",
    "in fact",
    "also",
    "still",
    "still",
    "despite that",
    "so",
    "",
    "skilled",
    "good",
    "changing",
    "interesting",
    "strong",
    "useful",
    "sturdy",
    "smooth",
    "cooperative",
    "interesting",
    "significant",
    "most",
    "lively",
    "important",
    "new",
    "advanced",
    "significant",
    "important",
    "novelty",
    "mix",
    "area",
    "space",
    "matches",
    "add",
    "explore",
    "start",
    "help",
    "use",
    "increase",
    "highlights",
    "use",
    "use",
    "explain",
    "transform",
    "support",
    "simplify",
    "shows",
    "to sum up",
    "to wrap up",
    "note that",
    "consider that",
    "note that",
    "but",
    "but",
    "fundamentally",
    "simply put",
    "this highlights the need for",
    "the main point is",
    "on a larger scale",
    "in most cases",
    "overall",
    "is often",
    "partially",
    "explain",
    "explains",
    "smooth compatibility",
    "expandable system",
    "practical insights",
    "practical insights",
    "informed decisions",
    "using",
    "this means",
    "helper that",
    "helper that",
    "this module handles",
    "this module handles",
    "we see that",
    "internally",
    "by default",
    "ultimately",
    "as noted",
    "for",
    "to",
    "a full",
    "offers a clean",
    "thorough",
    "comfortable",
    "production-ready",
];

static EXTRA_SPACES_RE: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(
        clippy::expect_used,
        reason = "static regex: pattern is validated by tests"
    )]
    Regex::new(r"[ ]{2,}").expect("valid spacing regex")
});
static SPACE_BEFORE_PUNCT_RE: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(
        clippy::expect_used,
        reason = "static regex: pattern is validated by tests"
    )]
    Regex::new(r"\s+([,.;:!?])").expect("valid punctuation regex")
});

static MATCHER: LazyLock<AhoCorasick> = LazyLock::new(|| {
    #[expect(
        clippy::expect_used,
        reason = "static matcher: patterns are validated by tests"
    )]
    AhoCorasick::builder()
        .ascii_case_insensitive(true)
        .build(SLOP_PATTERNS)
        .expect("valid lexical matcher patterns")
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
pub const fn corpus() -> &'static [&'static str] {
    &SLOP_PATTERNS
}

pub fn detect_file(path: impl AsRef<Path>) -> Result<Vec<Finding>, PapertowelError> {
    let path = path.as_ref();
    let content =
        fs::read_to_string(path).map_err(|error| PapertowelError::io_with_path(path, error))?;
    detect_in_text(path, &content, LexicalDetectionConfig::default())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexicalTransformResult {
    pub transformed_text: String,
    pub replacements_applied: usize,
    pub changed: bool,
}

pub fn transform_file(
    path: impl AsRef<Path>,
    dry_run: bool,
) -> Result<LexicalTransformResult, PapertowelError> {
    let path = path.as_ref();
    let original =
        fs::read_to_string(path).map_err(|error| PapertowelError::io_with_path(path, error))?;
    let transformed = transform_text(&original);

    if !dry_run && transformed.changed {
        fs::write(path, &transformed.transformed_text)
            .map_err(|error| PapertowelError::io_with_path(path, error))?;
    }

    Ok(transformed)
}

#[must_use]
pub fn transform_text(content: &str) -> LexicalTransformResult {
    let replacements_applied = MATCHER.find_iter(content).count();
    if replacements_applied == 0 {
        return LexicalTransformResult {
            transformed_text: content.to_owned(),
            replacements_applied: 0,
            changed: false,
        };
    }

    let replaced = MATCHER.replace_all(content, &SLOP_REPLACEMENTS);
    let cleaned = normalize_transformed_text(&replaced);
    let changed = cleaned != content;

    LexicalTransformResult {
        transformed_text: cleaned,
        replacements_applied,
        changed,
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "confidence score: bounded usize counts"
)]
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

fn normalize_transformed_text(content: &str) -> String {
    let mut normalized_lines = Vec::new();

    for line in content.lines() {
        let squashed = EXTRA_SPACES_RE.replace_all(line, " ");
        let punctuation = SPACE_BEFORE_PUNCT_RE.replace_all(&squashed, "$1");
        normalized_lines.push(punctuation.trim_end().to_owned());
    }

    normalized_lines.join("\n")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use crate::detection::finding::Severity;
    use crate::scrubber::lexical::{
        DETECTOR_NAME, LexicalDetectionConfig, corpus, detect_file, detect_in_text, transform_file,
        transform_text,
    };

    #[test]
    fn detector_name_is_stable() {
        assert_eq!(DETECTOR_NAME, "lexical");
    }

    #[test]
    fn corpus_contains_key_reference_phrase() {
        assert!(corpus().contains(&"delve"));
    }

    #[test]
    fn detect_in_text_returns_empty_for_sparse_terms() -> Result<(), Box<dyn std::error::Error>> {
        let findings = detect_in_text(
            "src/lib.rs",
            "This module is robust in exactly one spot.",
            LexicalDetectionConfig::default(),
        )?;

        assert!(findings.is_empty());
        Ok(())
    }

    #[test]
    fn detect_in_text_flags_clustered_slop() -> Result<(), Box<dyn std::error::Error>> {
        let sample = concat!(
            "this module provides a robust and seamless approach.\n",
            "it\'s worth noting that the design is comprehensive.\n",
            "to facilitate a vibrant experience, we delve into the details.\n",
        );

        let findings = detect_in_text("src/lib.rs", sample, LexicalDetectionConfig::default())?;

        assert_eq!(findings.len(), 1);
        let Some(first) = findings.first() else {
            return Err("expected first finding".into());
        };
        assert!(matches!(first.severity, Severity::Medium | Severity::High));
        assert!(first.line_range.is_some());
        Ok(())
    }

    #[test]
    fn detect_file_reads_and_processes_content() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = TempDir::new()?;
        let file_path = tmp.path().join("sample.rs");

        fs::write(
            &file_path,
            "this module provides a robust approach that is seamless and comprehensive. We delve into it.",
        )?;

        let findings = detect_file(&file_path)?;
        assert_eq!(findings.len(), 1);
        Ok(())
    }

    #[test]
    fn transform_text_rewrites_slop_phrases() {
        let sample = "this module provides a robust and seamless approach. It is robust.";
        let transformed = transform_text(sample);

        assert!(transformed.changed);
        assert!(transformed.replacements_applied >= 3);
        assert!(transformed.transformed_text.contains("this module"));
        assert!(
            !transformed
                .transformed_text
                .to_ascii_lowercase()
                .contains("robust")
        );
    }

    #[test]
    fn transform_file_honors_dry_run() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = TempDir::new()?;
        let file_path = tmp.path().join("sample.md");

        fs::write(&file_path, "A robust and seamless guide.")?;

        let transform_result = transform_file(&file_path, true)?;
        assert!(transform_result.changed);

        let disk_content = fs::read_to_string(&file_path)?;
        assert!(disk_content.contains("robust"));
        Ok(())
    }

    #[test]
    fn transform_text_returns_unchanged_for_clean_content() {
        let result = transform_text("fn hello() { println!(\"hello world\"); }");
        assert!(!result.changed, "clean Rust code should not be modified");
        assert_eq!(result.replacements_applied, 0);
    }

    #[test]
    fn transform_file_writes_when_not_dry_run() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = TempDir::new()?;
        let file_path = tmp.path().join("slop.md");
        fs::write(&file_path, "A robust and seamless guide.")?;
        let result = transform_file(&file_path, false)?;
        assert!(result.changed);
        let disk = fs::read_to_string(&file_path)?;
        // After real write, slop words are replaced
        assert!(!disk.to_ascii_lowercase().contains("robust and seamless"));
        Ok(())
    }

    #[test]
    fn build_line_range_no_offsets_returns_none() -> Result<(), Box<dyn std::error::Error>> {
        // Covers line 249: _ => Ok(None) in build_line_range when both offsets are None.
        use super::build_line_range;
        let result = build_line_range("some content", None, None)?;
        assert!(result.is_none(), "no offsets → no line range");
        Ok(())
    }
}
