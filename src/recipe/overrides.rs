//! Application of user-configured rule overrides to loaded recipes.
//!
//! Overrides are resolved at recipe-load time rather than at finding-report
//! time so that a single source of truth governs every downstream consumer:
//! the scan-time [`super::matcher::RecipeMatcher`], the scrub-time
//! [`super::scrubber::RecipeScrubber`], and the explainability reports. A
//! word disabled here is therefore never detected *and* never rewritten —
//! silencing a term without also pinning `scrub` from still replacing it.
//!
//! Two independent override sources are supported:
//!
//! - `[detectors.lexical].exclude_terms` — vocabulary-level suppression by literal term.
//!   Mirrors the hardcoded-matcher behaviour in [`crate::scrubber::lexical`], which
//!   historically applied only to `SLOP_PATTERNS` and silently ignored recipe-backed
//!   rules.
//! - `[rules].disable` / `[rules].severity` — rule-id-level overrides that address any
//!   rule, including built-in detectors, by the id reported in scan output.

use crate::config::{LexicalRulesConfig, RulesConfig};
use crate::detection::finding::{Finding, Severity};

use super::types::{LoadedRecipe, PhraseItem, WordItem};

/// Build the canonical rule id for a recipe word, matching
/// [`super::matcher`]'s emitted id.
#[must_use]
pub fn word_rule_id(recipe: &str, word: &str) -> String {
    format!("{recipe}:word:{word}")
}

/// Build the canonical rule id for a recipe phrase, matching
/// [`super::matcher`]'s emitted id.
#[must_use]
pub fn phrase_rule_id(recipe: &str, phrase: &str) -> String {
    format!("{}:phrase:{}", recipe, phrase.replace(' ', "-"))
}

/// Build the canonical rule id for a recipe regex, matching
/// [`super::matcher`]'s emitted id.
#[must_use]
pub fn regex_rule_id(recipe: &str, name: &str) -> String {
    format!("{recipe}:regex:{name}")
}

/// Build the canonical rule id for a recipe contextual pattern, matching
/// [`super::matcher`]'s emitted id.
#[must_use]
pub fn contextual_rule_id(recipe: &str, name: &str) -> String {
    format!("{recipe}:contextual:{name}")
}

fn normalize(term: &str, case_sensitive: bool) -> String {
    if case_sensitive {
        term.trim().to_owned()
    } else {
        term.trim().to_ascii_lowercase()
    }
}

/// Remove and re-grade rules in `recipes` according to the user's config.
///
/// `lexical` supplies `exclude_terms` / `case_sensitive`; `rules` supplies
/// id-based `disable` and `severity` overrides. Mutates `recipes` in place.
pub fn apply_recipe_overrides(
    recipes: &mut [LoadedRecipe],
    lexical: &LexicalRulesConfig,
    rules: &RulesConfig,
) {
    let excluded: Vec<String> = lexical
        .exclude_terms
        .iter()
        .map(|term| normalize(term, lexical.case_sensitive))
        .collect();

    for loaded in recipes.iter_mut() {
        let recipe = &mut loaded.recipe;
        let name = recipe.recipe.name.clone();

        if let Some(words) = recipe.patterns.words.as_mut() {
            words.items.retain(|item| {
                let word = item.word();
                if excluded.contains(&normalize(word, lexical.case_sensitive)) {
                    return false;
                }
                !rules.is_disabled(&word_rule_id(&name, word))
            });

            for item in &mut words.items {
                if let Some(severity) = rules.severity_override(&word_rule_id(&name, item.word())) {
                    item.set_severity(severity);
                }
            }
        }

        if let Some(phrases) = recipe.patterns.phrases.as_mut() {
            phrases.items.retain(|item| {
                let pattern = item.pattern();
                if excluded.contains(&normalize(pattern, lexical.case_sensitive)) {
                    return false;
                }
                !rules.is_disabled(&phrase_rule_id(&name, pattern))
            });

            for item in &mut phrases.items {
                if let Some(severity) =
                    rules.severity_override(&phrase_rule_id(&name, item.pattern()))
                {
                    item.set_severity(severity);
                }
            }
        }

        recipe
            .patterns
            .regex
            .retain(|pattern| !rules.is_disabled(&regex_rule_id(&name, &pattern.name)));
        for pattern in &mut recipe.patterns.regex {
            if let Some(severity) = rules.severity_override(&regex_rule_id(&name, &pattern.name)) {
                pattern.severity = Some(severity);
            }
        }

        recipe
            .patterns
            .contextual
            .retain(|pattern| !rules.is_disabled(&contextual_rule_id(&name, &pattern.name)));
        for pattern in &mut recipe.patterns.contextual {
            if let Some(severity) =
                rules.severity_override(&contextual_rule_id(&name, &pattern.name))
            {
                pattern.severity = Some(severity);
            }
        }
    }
}

/// Drop findings whose rule id is disabled and re-grade the rest.
///
/// This is the second line of defence for rule ids that do not originate from
/// a recipe — the built-in detectors (`SEC012`, `STYLOMETRY-001`,
/// `lexical.cluster`, and friends), which each emit a stable id.
pub fn apply_finding_overrides(findings: &mut Vec<Finding>, rules: &RulesConfig) {
    if rules.disable.is_empty() && rules.severity.is_empty() {
        return;
    }

    findings.retain(|finding| !rules.is_disabled(&finding.id));

    for finding in findings.iter_mut() {
        if let Some(severity) = rules.severity_override(&finding.id) {
            finding.severity = severity;
        }
    }
}

// ─── Item severity setters ───────────────────────────────────────────────────
//
// `WordItem` / `PhraseItem` are `untagged` enums, so a `Simple` variant has no
// severity field to write. Promoting one to the struct form keeps the
// serialized shape valid (`replacement` stays absent).

impl WordItem {
    fn set_severity(&mut self, severity: Severity) {
        if let Self::WithReplacement { severity: slot, .. } = self {
            *slot = Some(severity);
        } else if let Self::Simple(word) = self {
            *self = Self::WithReplacement {
                word: word.clone(),
                replacement: None,
                severity: Some(severity),
            };
        }
    }
}

impl PhraseItem {
    fn set_severity(&mut self, severity: Severity) {
        if let Self::WithMeta { severity: slot, .. } = self {
            *slot = Some(severity);
        } else if let Self::Simple(pattern) = self {
            *self = Self::WithMeta {
                pattern: pattern.clone(),
                suggestion: None,
                severity: Some(severity),
            };
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test assertions"
)]
mod tests {
    use super::*;
    use crate::config::RuleSeverity;
    use crate::detection::finding::FindingCategory;
    use crate::recipe::types::{RecipeMetadata, ScoringConfig};
    use std::collections::BTreeMap;

    fn loaded(name: &str, words: &[&str]) -> LoadedRecipe {
        LoadedRecipe {
            recipe: super::super::types::Recipe {
                recipe: RecipeMetadata {
                    name: name.to_owned(),
                    version: "1.0.0".to_owned(),
                    description: String::new(),
                    author: String::new(),
                    category: crate::recipe::types::RecipeCategory::default(),
                    default_severity: Severity::Low,
                    enabled: true,
                },
                patterns: super::super::types::RecipePatterns {
                    words: Some(super::super::types::WordPatterns {
                        enabled: true,
                        case_sensitive: false,
                        whole_word: true,
                        severity: Some(Severity::Low),
                        items: words
                            .iter()
                            .map(|w| WordItem::Simple((*w).to_owned()))
                            .collect(),
                    }),
                    phrases: None,
                    regex: Vec::new(),
                    contextual: Vec::new(),
                },
                scoring: ScoringConfig::default(),
            },
            source: super::super::types::RecipeSource::Builtin,
        }
    }

    fn words_of(recipes: &[LoadedRecipe]) -> Vec<String> {
        recipes[0]
            .recipe
            .patterns
            .words
            .as_ref()
            .map(|w| {
                w.items
                    .iter()
                    .map(WordItem::word)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn exclude_terms_removes_recipe_words() {
        let mut recipes = vec![loaded("slop-vocabulary", &["sturdy", "use", "explore"])];
        let lexical = LexicalRulesConfig {
            exclude_terms: vec!["use".to_owned()],
            ..LexicalRulesConfig::default()
        };
        apply_recipe_overrides(&mut recipes, &lexical, &RulesConfig::default());
        assert_eq!(words_of(&recipes), vec!["sturdy", "explore"]);
    }

    #[test]
    fn exclude_terms_is_case_insensitive_by_default() {
        let mut recipes = vec![loaded("slop-vocabulary", &["use", "explore"])];
        let lexical = LexicalRulesConfig {
            exclude_terms: vec!["use".to_owned()],
            ..LexicalRulesConfig::default()
        };
        apply_recipe_overrides(&mut recipes, &lexical, &RulesConfig::default());
        assert_eq!(words_of(&recipes), vec!["explore"]);
    }

    #[test]
    fn rules_disable_removes_word_by_id() {
        let mut recipes = vec![loaded("slop-vocabulary", &["sturdy", "use"])];
        let rules = RulesConfig {
            disable: vec![word_rule_id("slop-vocabulary", "use")],
            severity: BTreeMap::new(),
        };
        apply_recipe_overrides(&mut recipes, &LexicalRulesConfig::default(), &rules);
        assert_eq!(words_of(&recipes), vec!["sturdy"]);
    }

    #[test]
    fn rules_severity_overrides_word() {
        let mut recipes = vec![loaded("slop-vocabulary", &["sturdy"])];
        let mut severity = BTreeMap::new();
        severity.insert(word_rule_id("slop-vocabulary", "sturdy"), RuleSeverity::Low);
        let rules = RulesConfig {
            disable: Vec::new(),
            severity,
        };
        apply_recipe_overrides(&mut recipes, &LexicalRulesConfig::default(), &rules);
        let items = &recipes[0]
            .recipe
            .patterns
            .words
            .as_ref()
            .expect("words")
            .items;
        assert_eq!(items[0].severity(), Some(Severity::Low));
    }

    #[test]
    fn severity_override_promotes_simple_variant() {
        let mut item = WordItem::Simple("sturdy".to_owned());
        item.set_severity(Severity::High);
        assert_eq!(item.severity(), Some(Severity::High));
        assert_eq!(item.replacement(), None);
        assert_eq!(item.word(), "sturdy");
    }

    #[test]
    fn finding_overrides_drop_and_regrade() {
        let mut findings = vec![
            Finding::new(
                "SEC012",
                FindingCategory::Security,
                Severity::High,
                0.9,
                "a.py",
                "desc",
            )
            .expect("finding"),
            Finding::new(
                "lexical.cluster",
                FindingCategory::Lexical,
                Severity::Low,
                0.6,
                "b.rs",
                "desc",
            )
            .expect("finding"),
        ];

        let mut severity = BTreeMap::new();
        severity.insert("lexical.cluster".to_owned(), RuleSeverity::High);
        let rules = RulesConfig {
            disable: vec!["SEC012".to_owned()],
            severity,
        };

        apply_finding_overrides(&mut findings, &rules);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].id, "lexical.cluster");
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[test]
    fn phrase_rule_id_matches_matcher_format() {
        assert_eq!(
            phrase_rule_id("phrase-patterns", "Let's break this down"),
            "phrase-patterns:phrase:Let's-break-this-down"
        );
    }
}
