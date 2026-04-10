
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
 /// Word matcher with replacements: (pattern, replacement).
 word_ac: Option<AhoCorasick>,
 word_replacements: Vec<String>,

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
 for regex_pat in &recipe.patterns.regex {
 if regex_pat.auto_fixable
 && let Some(ref fix) = regex_pat.fix_pattern
 {
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

 // Collect contextual patterns with fix_pattern.
 for ctx_pat in &recipe.patterns.contextual {
 if ctx_pat.auto_fixable
 && ctx_pat.is_regex
 && let Some(ref fix) = ctx_pat.fix_pattern
 {
 match Regex::new(&ctx_pat.pattern) {
 Ok(re) => regex_patterns.push((re, fix.clone())),
 Err(e) => {
 tracing::warn!(
 pattern = %ctx_pat.pattern,
 error = %e,
 "invalid contextual regex pattern, skipping"
 );
 }
 }
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
 word_replacements,
 phrase_ac,
 phrase_replacements,
 regex_patterns,
 })
 }

 #[must_use]
 pub const fn has_patterns(&self) -> bool {
 self.word_ac.is_some() || self.phrase_ac.is_some() ||!self.regex_patterns.is_empty()
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

 if!dry_run && result.changed {
 fs::write(path, &result.transformed_text)
.map_err(|error| PapertowelError::io_with_path(path, error))?;
 }

 Ok(result)
 }

 #[must_use]
 pub fn transform_text(&self, content: &str) -> RecipeTransformResult {
 let mut text = content.to_owned();
 let mut total_replacements = 0_usize;

 // Apply word replacements.
 if let Some(ref ac) = self.word_ac {
 let count = ac.find_iter(&text).count();
 if count > 0 {
 text = ac.replace_all(&text, &self.word_replacements);
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

 let changed = text!= content;

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
 use crate::recipe::types::{
 PhraseItem, PhrasePatterns, Recipe, RecipeCategory, RecipeMetadata, RecipePatterns,
 RecipeSource, ScoringConfig, WordItem, WordPatterns,
 };
 use crate::detection::finding::Severity;

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