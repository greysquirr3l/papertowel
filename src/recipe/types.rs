use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::detection::finding::{FindingCategory, Severity};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
    /// Recipe metadata.
    pub recipe: RecipeMetadata,

    /// Pattern definitions.
    #[serde(default)]
    pub patterns: RecipePatterns,

    /// Scoring configuration.
    #[serde(default)]
    pub scoring: ScoringConfig,
}

/// Metadata about the recipe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeMetadata {
    pub name: String,

    #[serde(default = "default_version")]
    pub version: String,

    /// Human-readable description.
    #[serde(default)]
    pub description: String,

    /// Recipe author or source.
    #[serde(default)]
    pub author: String,

    #[serde(default)]
    pub category: RecipeCategory,

    #[serde(default)]
    pub default_severity: Severity,

    /// Whether this recipe is enabled by default.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_version() -> String {
    "1.0.0".to_owned()
}

const fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum RecipeCategory {
    #[default]
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
    Custom,
}

impl From<RecipeCategory> for FindingCategory {
    fn from(cat: RecipeCategory) -> Self {
        match cat {
            RecipeCategory::Comment => Self::Comment,
            RecipeCategory::Structure => Self::Structure,
            RecipeCategory::Readme => Self::Readme,
            RecipeCategory::Metadata => Self::Metadata,
            RecipeCategory::Workflow => Self::Workflow,
            RecipeCategory::Maintenance => Self::Maintenance,
            RecipeCategory::Promotion => Self::Promotion,
            RecipeCategory::NameCredibility => Self::NameCredibility,
            RecipeCategory::IdiomMismatch => Self::IdiomMismatch,
            RecipeCategory::TestPattern => Self::TestPattern,
            RecipeCategory::PromptLeakage => Self::PromptLeakage,
            RecipeCategory::CommitPattern => Self::CommitPattern,
            RecipeCategory::Lexical | RecipeCategory::Custom => Self::Lexical,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecipePatterns {
    /// Simple word matching via Aho-Corasick.
    #[serde(default)]
    pub words: Option<WordPatterns>,

    /// Phrase matching with optional suggestions.
    #[serde(default)]
    pub phrases: Option<PhrasePatterns>,

    /// Regex-based patterns.
    #[serde(default)]
    pub regex: Vec<RegexPattern>,

    /// Context-aware patterns (file-specific).
    #[serde(default)]
    pub contextual: Vec<ContextualPattern>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordPatterns {
    /// Whether this pattern group is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Case-sensitive matching.
    #[serde(default)]
    pub case_sensitive: bool,

    /// Match whole words only (word boundaries).
    #[serde(default = "default_true")]
    pub whole_word: bool,

    #[serde(default)]
    pub severity: Option<Severity>,

    #[serde(default)]
    pub items: Vec<WordItem>,
}

/// A single word pattern with optional replacement.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WordItem {
    /// Simple string word.
    Simple(String),

    /// Word with replacement.
    WithReplacement {
        word: String,

        /// Replacement text (empty string = delete).
        #[serde(default)]
        replacement: Option<String>,

        /// Severity override.
        #[serde(default)]
        severity: Option<Severity>,
    },
}

impl WordItem {
    /// Get the word string.
    pub fn word(&self) -> &str {
        match self {
            Self::Simple(s) => s,
            Self::WithReplacement { word, .. } => word,
        }
    }

    /// Get the replacement, if any.
    pub fn replacement(&self) -> Option<&str> {
        match self {
            Self::Simple(_) => None,
            Self::WithReplacement { replacement, .. } => replacement.as_deref(),
        }
    }

    /// Get severity override, if any.
    pub const fn severity(&self) -> Option<Severity> {
        match self {
            Self::Simple(_) => None,
            Self::WithReplacement { severity, .. } => *severity,
        }
    }
}

impl std::fmt::Display for WordItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Simple(s) => write!(f, "{s}"),
            Self::WithReplacement {
                word, replacement, ..
            } => {
                if let Some(repl) = replacement {
                    write!(f, "{word} → {repl}")
                } else {
                    write!(f, "{word}")
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhrasePatterns {
    /// Whether this pattern group is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default)]
    pub severity: Option<Severity>,

    #[serde(default)]
    pub items: Vec<PhraseItem>,
}

/// A single phrase pattern with optional fix suggestion.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PhraseItem {
    /// Simple string phrase.
    Simple(String),

    /// Phrase with metadata.
    WithMeta {
        #[serde(rename = "match")]
        pattern: String,

        /// Suggested replacement.
        #[serde(default)]
        suggestion: Option<String>,

        /// Severity override.
        #[serde(default)]
        severity: Option<Severity>,
    },
}

impl PhraseItem {
    /// Get the pattern string.
    pub fn pattern(&self) -> &str {
        match self {
            Self::Simple(s) => s,
            Self::WithMeta { pattern, .. } => pattern,
        }
    }

    /// Get the suggestion, if any.
    pub fn suggestion(&self) -> Option<&str> {
        match self {
            Self::Simple(_) => None,
            Self::WithMeta { suggestion, .. } => suggestion.as_deref(),
        }
    }

    /// Get severity override, if any.
    pub const fn severity(&self) -> Option<Severity> {
        match self {
            Self::Simple(_) => None,
            Self::WithMeta { severity, .. } => *severity,
        }
    }
}

/// A regex-based pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegexPattern {
    pub name: String,

    /// The regex pattern string.
    pub pattern: String,

    #[serde(default)]
    pub severity: Option<Severity>,

    /// Human-readable description.
    #[serde(default)]
    pub description: Option<String>,

    /// Suggested fix.
    #[serde(default)]
    pub suggestion: Option<String>,

    #[serde(default)]
    pub auto_fixable: bool,

    /// Replacement pattern (empty string = delete).
    #[serde(default)]
    pub fix_pattern: Option<String>,

    #[serde(default)]
    pub applies_to: Vec<String>,

    #[serde(default)]
    pub excludes: Vec<String>,
}

/// A context-aware pattern that only matches in specific files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextualPattern {
    pub name: String,

    pub applies_to: Vec<String>,

    pub pattern: String,

    /// Whether the pattern is a regex.
    #[serde(default)]
    pub is_regex: bool,

    #[serde(default)]
    pub severity: Option<Severity>,

    /// Human-readable description.
    #[serde(default)]
    pub description: Option<String>,

    /// Suggested fix.
    #[serde(default)]
    pub suggestion: Option<String>,

    #[serde(default)]
    pub auto_fixable: bool,

    /// Replacement pattern.
    #[serde(default)]
    pub fix_pattern: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoringConfig {
    #[serde(default = "default_cluster_threshold")]
    pub cluster_threshold: usize,

    #[serde(default = "default_cluster_range")]
    pub cluster_range_lines: usize,

    /// Severity boost when cluster threshold is met.
    #[serde(default)]
    pub cluster_severity_boost: Option<Severity>,

    #[serde(default = "default_confidence")]
    pub base_confidence: f32,
}

impl Default for ScoringConfig {
    fn default() -> Self {
        Self {
            cluster_threshold: default_cluster_threshold(),
            cluster_range_lines: default_cluster_range(),
            cluster_severity_boost: None,
            base_confidence: default_confidence(),
        }
    }
}

const fn default_cluster_threshold() -> usize {
    3
}

const fn default_cluster_range() -> usize {
    10
}

const fn default_confidence() -> f32 {
    0.7
}

#[derive(Debug, Clone)]
pub enum RecipePattern {
    Word {
        word: String,
        case_sensitive: bool,
        whole_word: bool,
        severity: Severity,
    },
    Phrase {
        phrase: String,
        suggestion: Option<String>,
        severity: Severity,
    },
    Regex(RegexPattern),
    Contextual(ContextualPattern),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecipeSource {
    /// Embedded in the binary.
    Builtin,
    UserGlobal(PathBuf),
    RepoLocal(PathBuf),
}

impl std::fmt::Display for RecipeSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Builtin => write!(f, "builtin"),
            Self::UserGlobal(p) => write!(f, "user:{}", p.display()),
            Self::RepoLocal(p) => write!(f, "repo:{}", p.display()),
        }
    }
}

/// A loaded recipe with its source.
#[derive(Debug, Clone)]
pub struct LoadedRecipe {
    pub recipe: Recipe,
    pub source: RecipeSource,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_recipe() {
        let toml = r#"
[recipe]
name = "test"

[patterns.words]
items = ["foo", "bar"]
"#;
        let recipe: Recipe = toml::from_str(toml).expect("parse failed");
        assert_eq!(recipe.recipe.name, "test");
        assert_eq!(
            recipe.patterns.words.as_ref().map(|w| w.items.len()),
            Some(2)
        );
    }

    #[test]
    fn parse_phrase_with_suggestion() {
        let toml = r#"
[recipe]
name = "phrases"

[patterns.phrases]
items = [
 "simple phrase",
 { match = "ultimately", suggestion = "ultimately" },
]
"#;
        let recipe: Recipe = toml::from_str(toml).expect("parse failed");
        let phrases = recipe.patterns.phrases.expect("phrases missing");
        assert_eq!(phrases.items.len(), 2);
        assert_eq!(phrases.items[0].pattern(), "simple phrase");
        assert_eq!(phrases.items[1].suggestion(), Some("ultimately"));
    }

    #[test]
    fn parse_regex_pattern() {
        let toml = r#"
[recipe]
name = "regex-test"

[[patterns.regex]]
name = "verbose-comment"
pattern = '^\s*//\s*This function'
severity = "Medium"
auto_fixable = true
fix_pattern = ""
"#;
        let recipe: Recipe = toml::from_str(toml).expect("parse failed");
        assert_eq!(recipe.patterns.regex.len(), 1);
        assert!(recipe.patterns.regex[0].auto_fixable);
    }
}
