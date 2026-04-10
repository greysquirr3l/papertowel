use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use regex::Regex;

use crate::domain::errors::PapertowelError;

use super::types::LoadedRecipe;

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

#[derive(Debug, Clone)]
pub struct RecipeTransformResult {
    pub transformed_text: String,
    /// Number of replacements applied.
    pub replacements_applied: usize,
    /// Whether the content changed.
    pub changed: bool,
}

#[derive(Debug)]
pub struct RecipeScrubber {
    /// Word matcher with replacements.
    word_ac: Option<AhoCorasick>,
    word_patterns: Vec<String>,
    word_replacements: Vec<String>,
    /// Per-word flags: (`whole_word`, `case_sensitive`).
    word_flags: Vec<(bool, bool)>,

    /// Phrase matcher with replacements: (pattern, replacement).
    phrase_ac: Option<AhoCorasick>,
    phrase_replacements: Vec<String>,

    /// Regex patterns with replacements: (regex, replacement).
    regex_patterns: Vec<(Regex, String)>,
}

impl RecipeScrubber {
    pub fn compile(recipes: Vec<LoadedRecipe>) -> Result<Self, PapertowelError> {
        let mut word_patterns: Vec<String> = Vec::new();
        let mut word_replacements: Vec<String> = Vec::new();
        let mut word_flags: Vec<(bool, bool)> = Vec::new();
        let mut phrase_patterns: Vec<String> = Vec::new();
        let mut phrase_replacements: Vec<String> = Vec::new();
        let mut regex_patterns: Vec<(Regex, String)> = Vec::new();

        for loaded in recipes {
            let recipe = loaded.recipe;

            // Collect word patterns with replacements.
            if let Some(ref words) = recipe.patterns.words
                && words.enabled
            {
                for item in &words.items {
                    if let Some(replacement) = item.replacement() {
                        word_patterns.push(item.word().to_owned());
                        word_replacements.push(replacement.to_owned());
                        word_flags.push((words.whole_word, words.case_sensitive));
                    }
                }
            }

            // Collect phrase patterns with suggestions.
            if let Some(ref phrases) = recipe.patterns.phrases
                && phrases.enabled
            {
                for item in &phrases.items {
                    if let Some(suggestion) = item.suggestion() {
                        phrase_patterns.push(item.pattern().to_owned());
                        phrase_replacements.push(suggestion.to_owned());
                    }
                }
            }

            // Collect regex patterns with fix_pattern.
            // Skip patterns with applies_to/excludes: the scrubber operates on raw text and
            // cannot enforce file-path gating at transform time.
            for regex_pat in &recipe.patterns.regex {
                if regex_pat.auto_fixable
                    && let Some(ref fix) = regex_pat.fix_pattern
                {
                    if !regex_pat.applies_to.is_empty() || !regex_pat.excludes.is_empty() {
                        tracing::debug!(
                            pattern = %regex_pat.pattern,
                            "skipping auto-fix regex with applies_to/excludes: path filtering not                              available in text-only transform"
                        );
                        continue;
                    }
                    match Regex::new(&regex_pat.pattern) {
                        Ok(re) => regex_patterns.push((re, fix.clone())),
                        Err(e) => {
                            tracing::warn!(
                            pattern = %regex_pat.pattern,
                            error = %e,
                            "invalid regex pattern, skipping"
                            );
                        }
                    }
                }
            }

            // Contextual patterns are file-scoped via `applies_to`; the scrubber cannot enforce
            // those restrictions without a path, so contextual auto-fix patterns are skipped here.
            for ctx_pat in &recipe.patterns.contextual {
                if ctx_pat.auto_fixable && ctx_pat.is_regex && ctx_pat.fix_pattern.is_some() {
                    tracing::debug!(
                        pattern = %ctx_pat.pattern,
                        "skipping contextual auto-fix: file-scoped `applies_to` cannot be                          enforced in text-only transform"
                    );
                }
            }
        }

        // Build Aho-Corasick matchers.
        let word_ac = if word_patterns.is_empty() {
            None
        } else {
            Some(
                AhoCorasickBuilder::new()
                    .ascii_case_insensitive(true)
                    .match_kind(MatchKind::LeftmostLongest)
                    .build(&word_patterns)
                    .map_err(|e| {
                        PapertowelError::Config(format!("failed to build word scrubber: {e}"))
                    })?,
            )
        };

        let phrase_ac = if phrase_patterns.is_empty() {
            None
        } else {
            Some(
                AhoCorasickBuilder::new()
                    .ascii_case_insensitive(true)
                    .match_kind(MatchKind::LeftmostLongest)
                    .build(&phrase_patterns)
                    .map_err(|e| {
                        PapertowelError::Config(format!("failed to build phrase scrubber: {e}"))
                    })?,
            )
        };

        Ok(Self {
            word_ac,
            word_patterns,
            word_replacements,
            word_flags,
            phrase_ac,
            phrase_replacements,
            regex_patterns,
        })
    }

    #[must_use]
    pub const fn has_patterns(&self) -> bool {
        self.word_ac.is_some() || self.phrase_ac.is_some() || !self.regex_patterns.is_empty()
    }

    pub fn transform_file(
        &self,
        path: impl AsRef<Path>,
        dry_run: bool,
    ) -> Result<RecipeTransformResult, PapertowelError> {
        let path = path.as_ref();
        let original =
            fs::read_to_string(path).map_err(|error| PapertowelError::io_with_path(path, error))?;

        let result = self.transform_text(&original);

        if !dry_run && result.changed {
            fs::write(path, &result.transformed_text)
                .map_err(|error| PapertowelError::io_with_path(path, error))?;
        }

        Ok(result)
    }

    #[must_use]
    pub fn transform_text(&self, content: &str) -> RecipeTransformResult {
        let mut text = content.to_owned();
        let mut total_replacements = 0_usize;

        // Apply word replacements with whole-word and case-sensitive boundary checks.
        if let Some(ref ac) = self.word_ac {
            let bytes = text.as_bytes();
            let mut new_text = String::with_capacity(text.len());
            let mut last_end = 0_usize;
            let mut count = 0_usize;

            for mat in ac.find_iter(&text) {
                let idx = mat.pattern().as_usize();
                let start = mat.start();
                let end = mat.end();
                let Some(replacement) = self.word_replacements.get(idx) else {
                    continue;
                };
                let (whole_word, case_sensitive) =
                    self.word_flags.get(idx).copied().unwrap_or_default();

                // Skip matches that violate whole-word boundaries.
                // `_` is treated as a word character so patterns don't match
                // inside snake_case identifiers like `robust_solution`.
                if whole_word {
                    let is_word_byte = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
                    let start_ok =
                        start == 0 || !bytes.get(start - 1).is_some_and(|&b| is_word_byte(b));
                    let end_ok =
                        end == bytes.len() || !bytes.get(end).is_some_and(|&b| is_word_byte(b));
                    if !start_ok || !end_ok {
                        continue;
                    }
                }

                // For case-sensitive patterns, verify the matched slice is an exact case match.
                // (The AC is built case-insensitively; this post-filters case-sensitive patterns.)
                if case_sensitive
                    && let Some(pattern) = self.word_patterns.get(idx)
                    && text.get(start..end) != Some(pattern.as_str())
                {
                    continue;
                }

                new_text.push_str(&text[last_end..start]);
                new_text.push_str(replacement);
                last_end = end;
                count += 1;
            }
            new_text.push_str(&text[last_end..]);

            if count > 0 {
                text = new_text;
                total_replacements += count;
            }
        }

        // Apply phrase replacements.
        if let Some(ref ac) = self.phrase_ac {
            let count = ac.find_iter(&text).count();
            if count > 0 {
                text = ac.replace_all(&text, &self.phrase_replacements);
                total_replacements += count;
            }
        }

        // Apply regex replacements.
        for (regex, replacement) in &self.regex_patterns {
            let matches = regex.find_iter(&text).count();
            if matches > 0 {
                text = regex.replace_all(&text, replacement).into_owned();
                total_replacements += matches;
            }
        }

        // Normalize if we made changes.
        if total_replacements > 0 {
            text = normalize_transformed_text(&text);
        }

        let changed = text != content;

        RecipeTransformResult {
            transformed_text: text,
            replacements_applied: total_replacements,
            changed,
        }
    }
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
    use super::*;
    use crate::detection::finding::Severity;
    use crate::recipe::types::{
        PhraseItem, PhrasePatterns, Recipe, RecipeCategory, RecipeMetadata, RecipePatterns,
        RecipeSource, ScoringConfig, WordItem, WordPatterns,
    };

    fn make_test_recipe() -> LoadedRecipe {
        LoadedRecipe {
            recipe: Recipe {
                recipe: RecipeMetadata {
                    name: "test".to_owned(),
                    version: "1.0.0".to_owned(),
                    description: String::new(),
                    author: String::new(),
                    category: RecipeCategory::Lexical,
                    default_severity: Severity::Low,
                    enabled: true,
                },
                patterns: RecipePatterns {
                    words: Some(WordPatterns {
                        enabled: true,
                        case_sensitive: false,
                        whole_word: true,
                        severity: Some(Severity::Low),
                        items: vec![
                            WordItem::WithReplacement {
                                word: "utilize".to_owned(),
                                replacement: Some("use".to_owned()),
                                severity: None,
                            },
                            WordItem::WithReplacement {
                                word: "robust".to_owned(),
                                replacement: Some("sturdy".to_owned()),
                                severity: None,
                            },
                        ],
                    }),
                    phrases: Some(PhrasePatterns {
                        enabled: true,
                        severity: Some(Severity::Medium),
                        items: vec![PhraseItem::WithMeta {
                            pattern: "it's worth noting that".to_owned(),
                            suggestion: Some("note:".to_owned()),
                            severity: None,
                        }],
                    }),
                    regex: vec![],
                    contextual: vec![],
                },
                scoring: ScoringConfig::default(),
            },
            source: RecipeSource::Builtin,
        }
    }

    #[test]
    fn scrubber_compiles_recipes() {
        let scrubber = RecipeScrubber::compile(vec![make_test_recipe()]).unwrap();
        assert!(scrubber.has_patterns());
    }

    #[test]
    fn scrubber_transforms_words() {
        let scrubber = RecipeScrubber::compile(vec![make_test_recipe()]).unwrap();
        let result = scrubber.transform_text("We utilize a robust solution.");

        assert!(result.changed);
        assert_eq!(result.replacements_applied, 2);
        assert_eq!(result.transformed_text, "We use a sturdy solution.");
    }

    #[test]
    fn scrubber_transforms_phrases() {
        let scrubber = RecipeScrubber::compile(vec![make_test_recipe()]).unwrap();
        let result = scrubber.transform_text("It's worth noting that this works.");

        assert!(result.changed);
        assert_eq!(result.replacements_applied, 1);
        assert_eq!(result.transformed_text, "note: this works.");
    }

    #[test]
    fn scrubber_normalizes_spacing() {
        let scrubber = RecipeScrubber::compile(vec![make_test_recipe()]).unwrap();
        // After replacing "utilize" with "use", spacing should be normalized.
        let result = scrubber.transform_text("We utilize  things.");

        assert!(result.changed);
        assert_eq!(result.transformed_text, "We use things.");
    }

    #[test]
    fn scrubber_no_change_when_no_matches() {
        let scrubber = RecipeScrubber::compile(vec![make_test_recipe()]).unwrap();
        let result = scrubber.transform_text("This is normal text.");

        assert!(!result.changed);
        assert_eq!(result.replacements_applied, 0);
        assert_eq!(result.transformed_text, "This is normal text.");
    }
}
