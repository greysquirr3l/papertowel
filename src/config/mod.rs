use std::fs;
use std::path::Path;

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde::{Deserialize, Serialize};

use crate::domain::errors::PapertowelError;

/// Name of the repo-level configuration file.
pub const CONFIG_FILE_NAME: &str = ".papertowel.toml";
/// Name of the gitignore-syntax path exclusion file.
pub const IGNORE_FILE_NAME: &str = ".papertowelignore";

// ─── Enums ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ScrubberAggression {
 Gentle,
 #[default]
 Moderate,
 Aggressive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MinimumSeverity {
 Low,
 #[default]
 Medium,
 High,
}

// ─── Sub-configs ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DetectorConfig {
 pub lexical: bool,
 pub comments: bool,
 pub structure: bool,
 pub readme: bool,
 pub metadata: bool,
 pub commit_pattern: bool,
 pub tests: bool,
 pub workflow: bool,
 pub maintenance: bool,
 pub promotion: bool,
 pub name_credibility: bool,
 pub idiom_mismatch: bool,
 pub prompt: bool,
}

impl Default for DetectorConfig {
 fn default() -> Self {
 Self {
 lexical: true,
 comments: true,
 structure: true,
 readme: true,
 metadata: true,
 commit_pattern: true,
 tests: true,
 workflow: true,
 maintenance: true,
 promotion: true,
 name_credibility: true,
 idiom_mismatch: true,
 prompt: true,
 }
 }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SeverityConfig {
 pub minimum: MinimumSeverity,
}

impl Default for SeverityConfig {
 fn default() -> Self {
 Self {
 minimum: MinimumSeverity::Medium,
 }
 }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScrubberConfig {
 pub aggression: ScrubberAggression,
}

impl Default for ScrubberConfig {
 fn default() -> Self {
 Self {
 aggression: ScrubberAggression::Moderate,
 }
 }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WringerProjectConfig {
 pub default_persona: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ExcludeConfig {
 pub paths: Vec<String>,
}

///
/// All sections are optional; missing sections use their `Default`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ProjectConfig {
 pub detectors: DetectorConfig,
 pub severity: SeverityConfig,
 pub scrubber: ScrubberConfig,
 pub wringer: WringerProjectConfig,
 pub exclude: ExcludeConfig,
}

// ─── Public API ──────────────────────────────────────────────────────────────

pub fn load_config(repo_root: impl AsRef<Path>) -> Result<ProjectConfig, PapertowelError> {
 let path = repo_root.as_ref().join(CONFIG_FILE_NAME);

 if!path.exists() {
 return Ok(ProjectConfig::default());
 }

 let text =
 fs::read_to_string(&path).map_err(|error| PapertowelError::io_with_path(&path, error))?;
 toml::from_str(&text).map_err(PapertowelError::TomlDeserialize)
}

pub fn save_config(
 repo_root: impl AsRef<Path>,
 config: &ProjectConfig,
) -> Result<(), PapertowelError> {
 let path = repo_root.as_ref().join(CONFIG_FILE_NAME);
 let text = toml::to_string_pretty(config).map_err(PapertowelError::TomlSerialize)?;
 fs::write(&path, text).map_err(|error| PapertowelError::io_with_path(&path, error))
}

/// Build a [`Gitignore`] matcher from:
/// 1. Patterns listed in `config.exclude.paths`
///
/// "nothing is ignored".
pub fn build_ignore_matcher(
 repo_root: impl AsRef<Path>,
 config: &ProjectConfig,
) -> Result<Option<Gitignore>, PapertowelError> {
 let repo_root = repo_root.as_ref();
 let mut builder = GitignoreBuilder::new(repo_root);

 // Inline patterns from [exclude] section
 for pattern in &config.exclude.paths {
 builder
.add_line(None, pattern)
.map_err(|e| PapertowelError::Config(e.to_string()))?;
 }

 let ignore_file = repo_root.join(IGNORE_FILE_NAME);
 if ignore_file.exists() {
 builder.add(ignore_file);
 }

 // If nothing was added, skip building the matcher entirely
 if config.exclude.paths.is_empty() &&!repo_root.join(IGNORE_FILE_NAME).exists() {
 return Ok(None);
 }

 let gitignore = builder
.build()
.map_err(|e| PapertowelError::Config(e.to_string()))?;

 Ok(Some(gitignore))
}

pub fn is_ignored(matcher: &Gitignore, root: &Path, path: &Path, is_dir: bool) -> bool {
 let relative = path.strip_prefix(root).unwrap_or(path);
 matcher
.matched_path_or_any_parents(relative, is_dir)
.is_ignore()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
 use std::io::Write;

 use tempfile::TempDir;

 use super::{
 DetectorConfig, ProjectConfig, ScrubberAggression, SeverityConfig, build_ignore_matcher,
 is_ignored, load_config, save_config,
 };

 fn scratch() -> TempDir {
 tempfile::tempdir().expect("tempdir")
 }

 #[test]
 fn missing_config_returns_default() {
 let dir = scratch();
 let config = load_config(dir.path()).expect("load_config");
 assert_eq!(config, ProjectConfig::default());
 }

 #[test]
 fn config_roundtrips_toml() {
 let dir = scratch();
 let mut config = ProjectConfig::default();
 config.detectors.lexical = false;
 config.scrubber.aggression = ScrubberAggression::Aggressive;

 save_config(dir.path(), &config).expect("save_config");
 let loaded = load_config(dir.path()).expect("load_config");
 assert_eq!(loaded, config);
 }

 #[test]
 fn partial_toml_uses_defaults_for_missing_sections() {
 let dir = scratch();
 let partial = r#"
[scrubber]
aggression = "gentle"
"#;
 std::fs::write(dir.path().join(".papertowel.toml"), partial).expect("write");
 let config = load_config(dir.path()).expect("load_config");
 assert_eq!(config.scrubber.aggression, ScrubberAggression::Gentle);
 assert_eq!(config.detectors, DetectorConfig::default());
 assert_eq!(config.severity, SeverityConfig::default());
 }

 #[test]
 fn no_ignore_patterns_returns_none_matcher() {
 let dir = scratch();
 let config = ProjectConfig::default();
 let matcher = build_ignore_matcher(dir.path(), &config).expect("build");
 assert!(matcher.is_none());
 }

 #[test]
 fn inline_exclude_pattern_ignores_vendor() {
 let dir = scratch();
 let mut config = ProjectConfig::default();
 config.exclude.paths.push("vendor/**".to_owned());

 let matcher = build_ignore_matcher(dir.path(), &config)
.expect("build")
.expect("should produce a matcher");

 let vendor_path = dir.path().join("vendor").join("lib.rs");
 assert!(
 is_ignored(&matcher, dir.path(), &vendor_path, false),
 "vendor/lib.rs should be ignored"
 );
 }

 #[test]
 fn papertowelignore_file_is_respected() {
 let dir = scratch();
 let mut f =
 std::fs::File::create(dir.path().join(".papertowelignore")).expect("create ignore");
 writeln!(f, "generated/").expect("write");
 drop(f);

 let config = ProjectConfig::default();
 let matcher = build_ignore_matcher(dir.path(), &config)
.expect("build")
.expect("should produce a matcher");

 let generated_path = dir.path().join("generated").join("schema.rs");
 assert!(
 is_ignored(&matcher, dir.path(), &generated_path, false),
 "generated/schema.rs should be ignored"
 );
 }

 #[test]
 fn non_ignored_path_passes_through() {
 let dir = scratch();
 let mut config = ProjectConfig::default();
 config.exclude.paths.push("vendor/**".to_owned());

 let matcher = build_ignore_matcher(dir.path(), &config)
.expect("build")
.expect("matcher");

 let src_path = dir.path().join("src").join("lib.rs");
 assert!(
!is_ignored(&matcher, dir.path(), &src_path, false),
 "src/lib.rs should not be ignored"
 );
 }
}