use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use aho_corasick::AhoCorasick;

use crate::config::LexicalRulesConfig;
use crate::detection::finding::{Finding, FindingCategory, LineRange, Severity};
use crate::domain::errors::PapertowelError;

pub const DETECTOR_NAME: &str = "lexical";
pub const MAX_CUSTOM_LEXICAL_ENTRIES: usize = 256;

pub const SLOP_PATTERNS: &[&str] = &[
    "accordingly",
    "additionally",
    "arguably",
    "certainly",
    "consequently",
    "hence",
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
    "comprehensive",
    "ergonomic",
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
    "leveraging",
    "it\'s important to note",
    "it\'s important to consider",
    "it\'s worth noting that",
    "on the contrary",
    "that being said",
    "at its core",
    "to put it simply",
    "generally speaking",
    "broadly speaking",
    "to some extent",
    "from a broader perspective",
    "a testament to",
    "in summary",
    "in conclusion",
    "this underscores the importance of",
    "a key takeaway is",
    "shed light on",
    "sheds light on",
    "seamless integration",
    "scalable solution",
    "actionable insights",
    "data-driven insights",
    "data-driven decisions",
    "under the hood",
    "out of the box",
    "at the end of the day",
    "ready for production",
    "as mentioned above",
    "for the sake of",
    "in order to",
    "provides a streamlined",
];

static MATCHER: LazyLock<AhoCorasick> = LazyLock::new(|| {
    #[expect(
        clippy::expect_used,
        reason = "static matcher: patterns are validated by tests"
    )]
    AhoCorasick::builder()
        .ascii_case_insensitive(true)
        .build(ACTIVE_SLOP_PATTERNS.as_slice())
        .expect("valid lexical matcher patterns")
});

static ACTIVE_SLOP_PATTERNS: LazyLock<Vec<&str>> = LazyLock::new(|| {
    SLOP_PATTERNS
        .iter()
        .filter(|term| !term.trim().is_empty())
        .copied()
        .collect()
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LexicalTermSource {
    Default,
    CustomTerm,
    CustomPhrase,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LexicalEffectiveTermExplainability {
    pub term: String,
    pub source: LexicalTermSource,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LexicalRulesExplainability {
    pub case_sensitive: bool,
    pub extra_terms: Vec<String>,
    pub extra_phrases: Vec<String>,
    pub exclude_terms: Vec<String>,
    pub effective_terms: Vec<String>,
    pub effective_entries: Vec<LexicalEffectiveTermExplainability>,
}

#[derive(Debug)]
pub struct LexicalMatcher {
    matcher: AhoCorasick,
    patterns: Vec<String>,
    explainability: LexicalRulesExplainability,
}

impl LexicalMatcher {
    pub fn from_rules(rules: &LexicalRulesConfig) -> Result<Self, PapertowelError> {
        let custom_count =
            rules.extra_terms.len() + rules.extra_phrases.len() + rules.exclude_terms.len();
        if custom_count > MAX_CUSTOM_LEXICAL_ENTRIES {
            return Err(PapertowelError::Validation(format!(
                "too many lexical custom entries ({custom_count}); max allowed is {MAX_CUSTOM_LEXICAL_ENTRIES}"
            )));
        }

        let mut excluded = BTreeSet::new();
        for term in &rules.exclude_terms {
            let cleaned = clean_entry(term)?;
            excluded.insert(normalize(&cleaned, rules.case_sensitive));
        }

        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        let mut patterns = Vec::new();
        let mut effective_entries = Vec::new();

        for &term in SLOP_PATTERNS {
            if term.trim().is_empty() {
                continue;
            }
            let normalized = normalize(term, rules.case_sensitive);
            if excluded.contains(&normalized) {
                continue;
            }
            seen.insert(normalized, term.to_owned());
            patterns.push(term.to_owned());
            effective_entries.push(LexicalEffectiveTermExplainability {
                term: term.to_owned(),
                source: LexicalTermSource::Default,
            });
        }

        for term in &rules.extra_terms {
            let cleaned = clean_entry(term)?;
            let normalized = normalize(&cleaned, rules.case_sensitive);

            if let Some(existing) = seen.get(&normalized) {
                return Err(PapertowelError::Validation(format!(
                    "duplicate lexical rule entry after normalization: '{cleaned}' conflicts with '{existing}'"
                )));
            }

            if excluded.contains(&normalized) {
                continue;
            }

            seen.insert(normalized, cleaned.clone());
            patterns.push(cleaned.clone());
            effective_entries.push(LexicalEffectiveTermExplainability {
                term: cleaned,
                source: LexicalTermSource::CustomTerm,
            });
        }

        for phrase in &rules.extra_phrases {
            let cleaned = clean_entry(phrase)?;
            let normalized = normalize(&cleaned, rules.case_sensitive);

            if let Some(existing) = seen.get(&normalized) {
                return Err(PapertowelError::Validation(format!(
                    "duplicate lexical rule entry after normalization: '{cleaned}' conflicts with '{existing}'"
                )));
            }

            if excluded.contains(&normalized) {
                continue;
            }

            seen.insert(normalized, cleaned.clone());
            patterns.push(cleaned.clone());
            effective_entries.push(LexicalEffectiveTermExplainability {
                term: cleaned,
                source: LexicalTermSource::CustomPhrase,
            });
        }

        let matcher = AhoCorasick::builder()
            .ascii_case_insensitive(!rules.case_sensitive)
            .build(&patterns)
            .map_err(|e| PapertowelError::Validation(format!("invalid lexical patterns: {e}")))?;

        Ok(Self {
            matcher,
            patterns: patterns.clone(),
            explainability: LexicalRulesExplainability {
                case_sensitive: rules.case_sensitive,
                extra_terms: rules.extra_terms.clone(),
                extra_phrases: rules.extra_phrases.clone(),
                exclude_terms: rules.exclude_terms.clone(),
                effective_terms: patterns,
                effective_entries,
            },
        })
    }

    #[must_use]
    pub const fn explainability(&self) -> &LexicalRulesExplainability {
        &self.explainability
    }

    pub fn detect_file(
        &self,
        path: impl AsRef<Path>,
        config: LexicalDetectionConfig,
    ) -> Result<Vec<Finding>, PapertowelError> {
        let path = path.as_ref();
        let content =
            fs::read_to_string(path).map_err(|error| PapertowelError::io_with_path(path, error))?;
        self.detect_in_text(path, &content, config)
    }

    pub fn detect_in_text(
        &self,
        file_path: impl Into<PathBuf>,
        content: &str,
        config: LexicalDetectionConfig,
    ) -> Result<Vec<Finding>, PapertowelError> {
        let file_path = file_path.into();

        let mut total_matches = 0_usize;
        let mut terms = BTreeSet::new();
        let mut first_offset = None;
        let mut last_offset = None;

        for candidate in self.matcher.find_iter(content) {
            total_matches += 1;

            if first_offset.is_none() {
                first_offset = Some(candidate.start());
            }
            last_offset = Some(candidate.end());

            let index = candidate.pattern().as_usize();
            if let Some(term) = self.patterns.get(index) {
                terms.insert(term.clone());
            }
        }

        build_finding(
            &file_path,
            content,
            config,
            total_matches,
            &terms,
            first_offset,
            last_offset,
        )
    }
}

fn clean_entry(raw: &str) -> Result<String, PapertowelError> {
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return Err(PapertowelError::Validation(
            "lexical config entries must not be empty".to_owned(),
        ));
    }
    Ok(cleaned.to_owned())
}

fn normalize(term: &str, case_sensitive: bool) -> String {
    if case_sensitive {
        term.to_owned()
    } else {
        term.to_ascii_lowercase()
    }
}

#[must_use]
pub const fn corpus() -> &'static [&'static str] {
    SLOP_PATTERNS
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
        if let Some(term) = ACTIVE_SLOP_PATTERNS.get(index) {
            terms.insert((*term).to_owned());
        }
    }

    build_finding(
        &file_path,
        content,
        config,
        total_matches,
        &terms,
        first_offset,
        last_offset,
    )
}

#[expect(
    clippy::cast_precision_loss,
    reason = "confidence score: bounded usize counts"
)]
fn build_finding(
    file_path: &Path,
    content: &str,
    config: LexicalDetectionConfig,
    total_matches: usize,
    terms: &BTreeSet<String>,
    first_offset: Option<usize>,
    last_offset: Option<usize>,
) -> Result<Vec<Finding>, PapertowelError> {
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
        file_path.to_path_buf(),
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

    use crate::config::LexicalRulesConfig;
    use crate::detection::finding::Severity;
    use crate::scrubber::lexical::{
        DETECTOR_NAME, LexicalDetectionConfig, LexicalMatcher, LexicalTermSource,
        MAX_CUSTOM_LEXICAL_ENTRIES, corpus, detect_file, detect_in_text,
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
    fn build_line_range_no_offsets_returns_none() -> Result<(), Box<dyn std::error::Error>> {
        // Covers line 249: _ => Ok(None) in build_line_range when both offsets are None.
        use super::build_line_range;
        let result = build_line_range("some content", None, None)?;
        assert!(result.is_none(), "no offsets → no line range");
        Ok(())
    }

    #[test]
    fn custom_rules_add_terms_and_exclude_defaults() -> Result<(), Box<dyn std::error::Error>> {
        let rules = LexicalRulesConfig {
            enabled: true,
            extra_terms: vec!["slopword".to_owned()],
            extra_phrases: vec!["it should be noted".to_owned()],
            exclude_terms: vec!["robust".to_owned()],
            case_sensitive: false,
        };

        let matcher = LexicalMatcher::from_rules(&rules)?;
        assert!(
            !matcher
                .explainability()
                .effective_terms
                .contains(&"robust".to_owned())
        );
        assert!(
            matcher
                .explainability()
                .effective_terms
                .contains(&"slopword".to_owned())
        );
        assert!(
            matcher
                .explainability()
                .effective_entries
                .iter()
                .any(|entry| entry.term == "slopword"
                    && entry.source == LexicalTermSource::CustomTerm)
        );
        assert!(
            matcher
                .explainability()
                .effective_entries
                .iter()
                .any(|entry| {
                    entry.term == "it should be noted"
                        && entry.source == LexicalTermSource::CustomPhrase
                })
        );
        Ok(())
    }

    #[test]
    fn duplicate_custom_entries_are_rejected() {
        let rules = LexicalRulesConfig {
            enabled: true,
            extra_terms: vec!["SlopWord".to_owned(), "slopword".to_owned()],
            ..LexicalRulesConfig::default()
        };

        let result = LexicalMatcher::from_rules(&rules);
        assert!(result.is_err());
        if let Err(err) = result {
            assert!(err.to_string().contains("duplicate lexical rule entry"));
        }
    }

    #[test]
    fn custom_entry_cap_is_enforced() {
        let rules = LexicalRulesConfig {
            extra_terms: std::iter::repeat_n("x".to_owned(), MAX_CUSTOM_LEXICAL_ENTRIES + 1)
                .collect(),
            ..LexicalRulesConfig::default()
        };
        let result = LexicalMatcher::from_rules(&rules);
        assert!(result.is_err());
        if let Err(err) = result {
            assert!(err.to_string().contains("too many lexical custom entries"));
        }
    }
}
