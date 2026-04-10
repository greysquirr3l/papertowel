
use std::collections::HashMap;
use std::path::Path;

use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::Regex;
use tracing::{debug, instrument, warn};

use crate::detection::finding::{Finding, FindingCategory, LineRange, Severity};
use crate::domain::errors::PapertowelError;

use super::types::{
 ContextualPattern, LoadedRecipe, Recipe, RegexPattern,
 ScoringConfig, WordPatterns,
};

#[derive(Debug)]
pub struct CompiledRecipe {
 /// Recipe name.
 pub name: String,

 /// Recipe category.
 pub category: FindingCategory,

 /// Default severity.
 pub default_severity: Severity,

 /// Scoring configuration.
 pub scoring: ScoringConfig,

 /// Compiled word matcher (Aho-Corasick).
 word_matcher: Option<CompiledWordMatcher>,

 /// Compiled phrase matcher (Aho-Corasick).
 phrase_matcher: Option<CompiledPhraseMatcher>,

 /// Compiled regex patterns.
 regex_patterns: Vec<CompiledRegex>,

 /// Compiled contextual patterns.
 contextual_patterns: Vec<CompiledContextual>,
}

#[derive(Debug)]
struct CompiledWordMatcher {
 ac: AhoCorasick,
 words: Vec<String>,
 case_sensitive: bool,
 whole_word: bool,
 severity: Severity,
}

#[derive(Debug)]
struct CompiledPhraseMatcher {
 ac: AhoCorasick,
 phrases: Vec<(String, Option<String>, Severity)>, // (pattern, suggestion, severity)
}

#[derive(Debug)]
struct CompiledRegex {
 name: String,
 regex: Regex,
 severity: Severity,
 description: Option<String>,
 suggestion: Option<String>,
 auto_fixable: bool,
 fix_pattern: Option<String>,
 applies_to: Option<GlobSet>,
 excludes: Option<GlobSet>,
}

#[derive(Debug)]
struct CompiledContextual {
 name: String,
 applies_to: GlobSet,
 pattern: ContextualMatcher,
 severity: Severity,
 description: Option<String>,
 suggestion: Option<String>,
 auto_fixable: bool,
 fix_pattern: Option<String>,
}

#[derive(Debug)]
enum ContextualMatcher {
 Literal(String),
 Regex(Regex),
}

/// Matches files against all loaded recipes.
#[derive(Debug)]
pub struct RecipeMatcher {
 compiled: Vec<CompiledRecipe>,
}

impl RecipeMatcher {
 #[instrument(skip(recipes))]
 pub fn compile(recipes: Vec<LoadedRecipe>) -> Result<Self, PapertowelError> {
 let mut compiled = Vec::with_capacity(recipes.len());

 for loaded in recipes {
 match Self::compile_recipe(loaded.recipe) {
 Ok(c) => compiled.push(c),
 Err(e) => {
 warn!(error = %e, "failed to compile recipe");
 }
 }
 }

 debug!(count = compiled.len(), "compiled recipes");
 Ok(Self { compiled })
 }

 /// Compile a single recipe.
 fn compile_recipe(recipe: Recipe) -> Result<CompiledRecipe, PapertowelError> {
 let name = recipe.recipe.name.clone();
 let category = recipe.recipe.category.into();
 let default_severity = recipe.recipe.default_severity;

 // Compile word patterns.
 let word_matcher = if let Some(words) = recipe.patterns.words {
 if words.enabled &&!words.items.is_empty() {
 Some(Self::compile_words(words, default_severity)?)
 } else {
 None
 }
 } else {
 None
 };

 // Compile phrase patterns.
 let phrase_matcher = if let Some(phrases) = recipe.patterns.phrases {
 if phrases.enabled &&!phrases.items.is_empty() {
 Some(Self::compile_phrases(phrases, default_severity)?)
 } else {
 None
 }
 } else {
 None
 };

 // Compile regex patterns.
 let regex_patterns = recipe
.patterns
.regex
.into_iter()
.filter_map(|r| Self::compile_regex(r, default_severity).ok())
.collect();

 // Compile contextual patterns.
 let contextual_patterns = recipe
.patterns
.contextual
.into_iter()
.filter_map(|c| Self::compile_contextual(c, default_severity).ok())
.collect();

 Ok(CompiledRecipe {
 name,
 category,
 default_severity,
 scoring: recipe.scoring,
 word_matcher,
 phrase_matcher,
 regex_patterns,
 contextual_patterns,
 })
 }

 fn compile_words(words: WordPatterns, default_severity: Severity) -> Result<CompiledWordMatcher, PapertowelError> {
 let patterns: Vec<String> = if words.case_sensitive {
 words.items.clone()
 } else {
 words.items.iter().map(|s| s.to_lowercase()).collect()
 };

 let ac = AhoCorasickBuilder::new()
.ascii_case_insensitive(!words.case_sensitive)
.match_kind(MatchKind::LeftmostLongest)
.build(&patterns)
.map_err(|e| PapertowelError::Config(format!("failed to build word matcher: {e}")))?;

 Ok(CompiledWordMatcher {
 ac,
 words: words.items,
 case_sensitive: words.case_sensitive,
 whole_word: words.whole_word,
 severity: words.severity.unwrap_or(default_severity),
 })
 }

 fn compile_phrases(phrases: super::types::PhrasePatterns, default_severity: Severity) -> Result<CompiledPhraseMatcher, PapertowelError> {
 let phrase_data: Vec<(String, Option<String>, Severity)> = phrases
.items
.iter()
.map(|item| {
 let sev = item.severity().unwrap_or(phrases.severity.unwrap_or(default_severity));
 (item.pattern().to_owned(), item.suggestion().map(|s| s.to_owned()), sev)
 })
.collect();

 let patterns: Vec<&str> = phrase_data.iter().map(|(p, _, _)| p.as_str()).collect();

 let ac = AhoCorasickBuilder::new()
.ascii_case_insensitive(true)
.match_kind(MatchKind::LeftmostLongest)
.build(&patterns)
.map_err(|e| PapertowelError::Config(format!("failed to build phrase matcher: {e}")))?;

 Ok(CompiledPhraseMatcher {
 ac,
 phrases: phrase_data,
 })
 }

 fn compile_regex(pattern: RegexPattern, default_severity: Severity) -> Result<CompiledRegex, PapertowelError> {
 let regex = Regex::new(&pattern.pattern)
.map_err(|e| PapertowelError::Config(format!("invalid regex {}: {e}", pattern.name)))?;

 let applies_to = if pattern.applies_to.is_empty() {
 None
 } else {
 Some(Self::build_globset(&pattern.applies_to)?)
 };

 let excludes = if pattern.excludes.is_empty() {
 None
 } else {
 Some(Self::build_globset(&pattern.excludes)?)
 };

 Ok(CompiledRegex {
 name: pattern.name,
 regex,
 severity: pattern.severity.unwrap_or(default_severity),
 description: pattern.description,
 suggestion: pattern.suggestion,
 auto_fixable: pattern.auto_fixable,
 fix_pattern: pattern.fix_pattern,
 applies_to,
 excludes,
 })
 }

 fn compile_contextual(pattern: ContextualPattern, default_severity: Severity) -> Result<CompiledContextual, PapertowelError> {
 let applies_to = Self::build_globset(&pattern.applies_to)?;

 let matcher = if pattern.is_regex {
 let regex = Regex::new(&pattern.pattern)
.map_err(|e| PapertowelError::Config(format!("invalid regex {}: {e}", pattern.name)))?;
 ContextualMatcher::Regex(regex)
 } else {
 ContextualMatcher::Literal(pattern.pattern)
 };

 Ok(CompiledContextual {
 name: pattern.name,
 applies_to,
 pattern: matcher,
 severity: pattern.severity.unwrap_or(default_severity),
 description: pattern.description,
 suggestion: pattern.suggestion,
 auto_fixable: pattern.auto_fixable,
 fix_pattern: pattern.fix_pattern,
 })
 }

 fn build_globset(patterns: &[String]) -> Result<GlobSet, PapertowelError> {
 let mut builder = GlobSetBuilder::new();
 for pattern in patterns {
 let glob = Glob::new(pattern)
.map_err(|e| PapertowelError::Config(format!("invalid glob {pattern}: {e}")))?;
 builder.add(glob);
 }
 builder
.build()
.map_err(|e| PapertowelError::Config(format!("failed to build globset: {e}")))
 }

 /// Scan file content and return findings.
 #[instrument(skip(self, content))]
 pub fn scan_file(&self, path: &Path, content: &str) -> Result<Vec<Finding>, PapertowelError> {
 let mut findings = Vec::new();
 let lines: Vec<&str> = content.lines().collect();

 for recipe in &self.compiled {
 // Word matches.
 if let Some(ref word_matcher) = recipe.word_matcher {
 findings.extend(self.match_words(recipe, word_matcher, path, &lines)?);
 }

 // Phrase matches.
 if let Some(ref phrase_matcher) = recipe.phrase_matcher {
 findings.extend(self.match_phrases(recipe, phrase_matcher, path, &lines)?);
 }

 // Regex matches.
 for regex in &recipe.regex_patterns {
 if!self.file_matches_globs(path, &regex.applies_to, &regex.excludes) {
 continue;
 }
 findings.extend(self.match_regex(recipe, regex, path, &lines)?);
 }

 // Contextual matches.
 for contextual in &recipe.contextual_patterns {
 if!contextual.applies_to.is_match(path) {
 continue;
 }
 findings.extend(self.match_contextual(recipe, contextual, path, &lines)?);
 }

 // Apply cluster scoring.
 self.apply_cluster_scoring(&mut findings, &recipe.scoring);
 }

 Ok(findings)
 }

 fn file_matches_globs(&self, path: &Path, applies_to: &Option<GlobSet>, excludes: &Option<GlobSet>) -> bool {
 // Check excludes first.
 if let Some(excl) = excludes {
 if excl.is_match(path) {
 return false;
 }
 }

 if let Some(applies) = applies_to {
 return applies.is_match(path);
 }

 // No restriction = matches all.
 true
 }

 fn match_words(
 &self,
 recipe: &CompiledRecipe,
 matcher: &CompiledWordMatcher,
 path: &Path,
 lines: &[&str],
 ) -> Result<Vec<Finding>, PapertowelError> {
 let mut findings = Vec::new();

 for (line_idx, line) in lines.iter().enumerate() {
 let search_line = if matcher.case_sensitive {
 line.to_string()
 } else {
 line.to_lowercase()
 };

 for mat in matcher.ac.find_iter(&search_line) {
 let word = &matcher.words[mat.pattern().as_usize()];

 // Check word boundaries if required.
 if matcher.whole_word {
 let start = mat.start();
 let end = mat.end();
 let bytes = search_line.as_bytes();

 let start_ok = start == 0 ||!bytes.get(start - 1).is_some_and(|b| b.is_ascii_alphanumeric());
 let end_ok = end == bytes.len() ||!bytes.get(end).is_some_and(|b| b.is_ascii_alphanumeric());

 if!start_ok ||!end_ok {
 continue;
 }
 }

 let mut finding = Finding::new(
 format!("{}:word:{}", recipe.name, word),
 recipe.category,
 matcher.severity,
 recipe.scoring.base_confidence,
 path,
 format!("slop vocabulary: '{}'", word),
 )?;
 finding.line_range = Some(LineRange::new(line_idx + 1, line_idx + 1)?);
 findings.push(finding);
 }
 }

 Ok(findings)
 }

 fn match_phrases(
 &self,
 recipe: &CompiledRecipe,
 matcher: &CompiledPhraseMatcher,
 path: &Path,
 lines: &[&str],
 ) -> Result<Vec<Finding>, PapertowelError> {
 let mut findings = Vec::new();

 for (line_idx, line) in lines.iter().enumerate() {
 for mat in matcher.ac.find_iter(line) {
 let (phrase, suggestion, severity) = &matcher.phrases[mat.pattern().as_usize()];

 let mut finding = Finding::new(
 format!("{}:phrase:{}", recipe.name, phrase.replace(' ', "-")),
 recipe.category,
 *severity,
 recipe.scoring.base_confidence,
 path,
 format!("slop phrase: '{}'", phrase),
 )?;
 finding.line_range = Some(LineRange::new(line_idx + 1, line_idx + 1)?);
 finding.suggestion = suggestion.clone();
 finding.auto_fixable = suggestion.is_some();
 findings.push(finding);
 }
 }

 Ok(findings)
 }

 fn match_regex(
 &self,
 recipe: &CompiledRecipe,
 pattern: &CompiledRegex,
 path: &Path,
 lines: &[&str],
 ) -> Result<Vec<Finding>, PapertowelError> {
 let mut findings = Vec::new();

 for (line_idx, line) in lines.iter().enumerate() {
 if pattern.regex.is_match(line) {
 let description = pattern
.description
.clone()
.unwrap_or_else(|| format!("regex match: {}", pattern.name));

 let mut finding = Finding::new(
 format!("{}:regex:{}", recipe.name, pattern.name),
 recipe.category,
 pattern.severity,
 recipe.scoring.base_confidence,
 path,
 description,
 )?;
 finding.line_range = Some(LineRange::new(line_idx + 1, line_idx + 1)?);
 finding.suggestion = pattern.suggestion.clone();
 finding.auto_fixable = pattern.auto_fixable;
 findings.push(finding);
 }
 }

 Ok(findings)
 }

 fn match_contextual(
 &self,
 recipe: &CompiledRecipe,
 pattern: &CompiledContextual,
 path: &Path,
 lines: &[&str],
 ) -> Result<Vec<Finding>, PapertowelError> {
 let mut findings = Vec::new();

 for (line_idx, line) in lines.iter().enumerate() {
 let matched = match &pattern.pattern {
 ContextualMatcher::Literal(s) => line.contains(s.as_str()),
 ContextualMatcher::Regex(r) => r.is_match(line),
 };

 if matched {
 let description = pattern
.description
.clone()
.unwrap_or_else(|| format!("contextual match: {}", pattern.name));

 let mut finding = Finding::new(
 format!("{}:contextual:{}", recipe.name, pattern.name),
 recipe.category,
 pattern.severity,
 recipe.scoring.base_confidence,
 path,
 description,
 )?;
 finding.line_range = Some(LineRange::new(line_idx + 1, line_idx + 1)?);
 finding.suggestion = pattern.suggestion.clone();
 finding.auto_fixable = pattern.auto_fixable;
 findings.push(finding);
 }
 }

 Ok(findings)
 }

 fn apply_cluster_scoring(&self, findings: &mut [Finding], config: &ScoringConfig) {
 if findings.len() < config.cluster_threshold {
 return;
 }

 let Some(boost_severity) = config.cluster_severity_boost else {
 return;
 };

 // Group findings by line proximity.
 let mut line_counts: HashMap<usize, usize> = HashMap::new();
 for finding in findings.iter() {
 if let Some(range) = finding.line_range {
 // Count in buckets of cluster_range_lines.
 let bucket = range.start / config.cluster_range_lines;
 *line_counts.entry(bucket).or_insert(0) += 1;
 }
 }

 // Find buckets that exceed threshold.
 let hot_buckets: Vec<usize> = line_counts
.into_iter()
.filter(|(_, count)| *count >= config.cluster_threshold)
.map(|(bucket, _)| bucket)
.collect();

 for finding in findings.iter_mut() {
 if let Some(range) = finding.line_range {
 let bucket = range.start / config.cluster_range_lines;
 if hot_buckets.contains(&bucket) {
 finding.severity = boost_severity;
 finding.confidence_score = (finding.confidence_score + 0.15).min(1.0);
 }
 }
 }
 }
}

#[cfg(test)]
mod tests {
 use super::*;
 use crate::recipe::types::{Recipe, RecipeCategory, RecipeMetadata, RecipePatterns, ScoringConfig};

 fn test_recipe() -> Recipe {
 Recipe {
 recipe: RecipeMetadata {
 name: "test".to_owned(),
 version: "1.0.0".to_owned(),
 description: String::new(),
 author: String::new(),
 category: RecipeCategory::Lexical,
 default_severity: Severity::Medium,
 enabled: true,
 },
 patterns: RecipePatterns {
 words: Some(WordPatterns {
 enabled: true,
 case_sensitive: false,
 whole_word: true,
 severity: None,
 items: vec!["sturdy".to_owned(), "use".to_owned()],
 }),
 phrases: None,
 regex: vec![],
 contextual: vec![],
 },
 scoring: ScoringConfig::default(),
 }
 }

 #[test]
 fn word_matching_works() {
 let recipe = test_recipe();
 let loaded = LoadedRecipe {
 recipe,
 source: super::super::types::RecipeSource::Builtin,
 };

 let matcher = RecipeMatcher::compile(vec![loaded]).unwrap();
 let content = "This is a sturdy solution. We use modern techniques.";
 let findings = matcher.scan_file(Path::new("test.rs"), content).unwrap();

 assert_eq!(findings.len(), 2);
 }

 #[test]
 fn whole_word_boundary_respected() {
 let recipe = test_recipe();
 let loaded = LoadedRecipe {
 recipe,
 source: super::super::types::RecipeSource::Builtin,
 };

 let matcher = RecipeMatcher::compile(vec![loaded]).unwrap();
 // "sturdyly" shouldn't match "sturdy" with whole_word=true
 let content = "This works sturdyly without issues.";
 let findings = matcher.scan_file(Path::new("test.rs"), content).unwrap();

 assert!(findings.is_empty());
 }
}