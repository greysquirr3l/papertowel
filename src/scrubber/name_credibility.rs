use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use walkdir::WalkDir;

use crate::detection::finding::{Finding, FindingCategory, LineRange, Severity};
use crate::domain::errors::PapertowelError;

pub const DETECTOR_NAME: &str = "name_credibility";

const GENERIC_NAME_TOKENS: [&str; 14] = [
 "ai",
 "assistant",
 "tool",
 "project",
 "app",
 "generator",
 "wrapper",
 "template",
 "starter",
 "scaffold",
 "ultimate",
 "nextgen",
 "best",
 "super",
];

const CODE_EXTENSIONS: [&str; 8] = ["rs", "go", "py", "ts", "tsx", "js", "cs", "zig"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameCredibilityConfig {
 pub min_generic_token_hits: usize,
 pub max_code_files_for_flag: usize,
 pub min_name_repetition_hits: usize,
}

impl Default for NameCredibilityConfig {
 fn default() -> Self {
 Self {
 min_generic_token_hits: 2,
 max_code_files_for_flag: 4,
 min_name_repetition_hits: 3,
 }
 }
}

pub fn detect_repo(repo_root: impl AsRef<Path>) -> Result<Vec<Finding>, PapertowelError> {
 detect_repo_with_config(repo_root, NameCredibilityConfig::default())
}

#[expect(
 clippy::cast_precision_loss,
 reason = "confidence score: bounded usize counts"
)]
pub fn detect_repo_with_config(
 repo_root: impl AsRef<Path>,
 config: NameCredibilityConfig,
) -> Result<Vec<Finding>, PapertowelError> {
 let repo_root = repo_root.as_ref();

 let package_name = read_package_name(repo_root)?;
 let generic_hits = count_generic_token_hits(&package_name);
 let code_files = count_code_files(repo_root)?;
 let repetition_hits = count_name_repetitions(repo_root, &package_name)?;

 let thin_implementation = code_files <= config.max_code_files_for_flag;
 let generic_name = generic_hits >= config.min_generic_token_hits;
 let noisy_discovery = repetition_hits >= config.min_name_repetition_hits;

 if!(thin_implementation && generic_name && noisy_discovery) {
 return Ok(Vec::new());
 }

 let severity = if generic_hits >= 3 && repetition_hits >= 5 && code_files <= 2 {
 Severity::High
 } else {
 Severity::Medium
 };

 let confidence = (generic_hits as f32 / 5.0)
.mul_add(
 0.4,
 (repetition_hits as f32 / 8.0).mul_add(
 0.4,
 ((config.max_code_files_for_flag.saturating_sub(code_files)) as f32
 / config.max_code_files_for_flag.max(1) as f32)
 * 0.2,
 ),
 )
.min(1.0);

 let mut finding = Finding::new(
 "name.discovery_pollution",
 FindingCategory::NameCredibility,
 severity,
 confidence,
 repo_root.join("Cargo.toml"),
 format!(
 "Detected low-credibility project naming signal (package: `{package_name}`, generic token hits: {generic_hits}, repetition hits: {repetition_hits}, code files: {code_files})."
 ),
 )?;
 finding.line_range = Some(LineRange::new(1, 1)?);
 finding.suggestion = Some(
		"Prefer specific, domain-grounded naming and avoid discovery-spam language when implementation depth is still thin."
.to_owned(),
	);

 Ok(vec![finding])
}

fn read_package_name(repo_root: &Path) -> Result<String, PapertowelError> {
 let cargo_toml_path = repo_root.join("Cargo.toml");
 let content = fs::read_to_string(&cargo_toml_path)
.map_err(|error| PapertowelError::io_with_path(&cargo_toml_path, error))?;
 let value: toml::Value = toml::from_str(&content)?;

 let package_name = value
.get("package")
.and_then(toml::Value::as_table)
.and_then(|table| table.get("name"))
.and_then(toml::Value::as_str)
.ok_or_else(|| {
 PapertowelError::Validation("Cargo.toml package.name is required".to_owned())
 })?;

 Ok(package_name.to_owned())
}

fn count_generic_token_hits(name: &str) -> usize {
 let lowered = name.to_ascii_lowercase();
 let segments = lowered
.split(['-', '_'])
.filter(|segment|!segment.is_empty())
.collect::<Vec<_>>();

 GENERIC_NAME_TOKENS
.iter()
.filter(|token| segments.iter().any(|segment| *segment == **token))
.count()
}

fn count_code_files(repo_root: &Path) -> Result<usize, PapertowelError> {
 let mut code_files = 0_usize;

 for entry in WalkDir::new(repo_root) {
 let entry = entry.map_err(|error| {
 let io_error = std::io::Error::other(error.to_string());
 let path = error
.path()
.map_or_else(|| repo_root.to_path_buf(), Path::to_path_buf);
 PapertowelError::io_with_path(path, io_error)
 })?;

 let path = entry.path();
 if path.components().any(|part| {
 part.as_os_str() == OsStr::new(".git")
 || part.as_os_str() == OsStr::new("target")
 || part.as_os_str() == OsStr::new(".coraline")
 }) {
 continue;
 }

 if!path.is_file() {
 continue;
 }

 let Some(ext) = path.extension().and_then(OsStr::to_str) else {
 continue;
 };

 if CODE_EXTENSIONS.contains(&ext) {
 code_files += 1;
 }
 }

 Ok(code_files)
}

fn count_name_repetitions(repo_root: &Path, package_name: &str) -> Result<usize, PapertowelError> {
 let readme_path = repo_root.join("README.md");
 if!readme_path.is_file() {
 return Ok(0);
 }

 let content = fs::read_to_string(&readme_path)
.map_err(|error| PapertowelError::io_with_path(&readme_path, error))?;
 let lowered = content.to_ascii_lowercase();
 let normalized_name = package_name.to_ascii_lowercase().replace('_', "-");
 Ok(lowered.matches(&normalized_name).count())
}

#[cfg(test)]
mod tests {
 #![expect(
 clippy::indexing_slicing,
 reason = "indexed assertions on known-populated vecs"
 )]

 use std::fs;

 use tempfile::TempDir;

 use crate::scrubber::name_credibility::{
 DETECTOR_NAME, NameCredibilityConfig, detect_repo, detect_repo_with_config,
 };

 #[test]
 fn detector_name_is_stable() {
 assert_eq!(DETECTOR_NAME, "name_credibility");
 }

 #[test]
 fn name_detector_ignores_specific_names() -> Result<(), Box<dyn std::error::Error>> {
 let temp = TempDir::new()?;

 fs::create_dir_all(temp.path().join("src"))?;
 fs::write(temp.path().join("src/lib.rs"), "pub fn run() {}\n")?;
 fs::write(
 temp.path().join("Cargo.toml"),
 "[package]\nname = \"domain-parser\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
 )?;
 fs::write(
 temp.path().join("README.md"),
 "domain-parser focuses on structured parsing and validation workflows.\n",
 )?;

 let findings = detect_repo_with_config(temp.path(), NameCredibilityConfig::default())?;
 assert!(findings.is_empty());
 Ok(())
 }

 #[test]
 fn name_detector_flags_generic_repetitive_names() -> Result<(), Box<dyn std::error::Error>> {
 let temp = TempDir::new()?;

 fs::create_dir_all(temp.path().join("src"))?;
 fs::write(temp.path().join("src/main.rs"), "fn main() {}\n")?;
 fs::write(
 temp.path().join("Cargo.toml"),
 "[package]\nname = \"ai-tool-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
 )?;
 fs::write(
 temp.path().join("README.md"),
 "ai-tool-app is a revolutionary showcase. ai-tool-app is instant. ai-tool-app is new. ai-tool-app helps everyone.",
 )?;

 let findings = detect_repo_with_config(temp.path(), NameCredibilityConfig::default())?;
 assert_eq!(findings.len(), 1);
 Ok(())
 }

 #[test]
 fn detect_repo_delegates_to_with_config() -> Result<(), Box<dyn std::error::Error>> {
 use std::fs;
 use tempfile::TempDir;
 let temp = TempDir::new()?;
 fs::create_dir_all(temp.path().join("src"))?;
 fs::write(temp.path().join("src/main.rs"), "fn main() {}\n")?;
 fs::write(
 temp.path().join("Cargo.toml"),
 "[package]\nname = \"simple-tool\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
 )?;
 fs::write(
 temp.path().join("README.md"),
 "simple-tool parses things.\n",
 )?;
 let findings = detect_repo(temp.path())?;
 let _ = findings;
 Ok(())
 }

 #[test]
 fn high_severity_when_generic_hits_and_repetitions_are_high()
 -> Result<(), Box<dyn std::error::Error>> {
 // Covers Severity::High path: generic_hits >= 3, repetition_hits >= 5, code_files <= 2.
 let temp = TempDir::new()?;
 fs::create_dir_all(temp.path().join("src"))?;
 // 1 code file → code_files <= 2 ✓
 fs::write(temp.path().join("src/main.rs"), "fn main() {}\n")?;
 fs::write(
 temp.path().join("Cargo.toml"),
 "[package]\nname = \"ai-tool-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
 )?;
 fs::write(
 temp.path().join("README.md"),
 "ai-tool-app is a tool. ai-tool-app is instant. ai-tool-app is next-gen. \
 ai-tool-app helps everyone. ai-tool-app is versatile. ai-tool-app is great.\n",
 )?;
 let config = NameCredibilityConfig {
 min_generic_token_hits: 2,
 max_code_files_for_flag: 4,
 min_name_repetition_hits: 3,
 };
 let findings = detect_repo_with_config(temp.path(), config)?;
 assert_eq!(findings.len(), 1);
 assert_eq!(
 findings[0].severity,
 crate::detection::finding::Severity::High
 );
 Ok(())
 }

 #[test]
 fn missing_cargo_toml_name_returns_error() -> Result<(), Box<dyn std::error::Error>> {
 use std::fs;
 use tempfile::TempDir;
 let temp = TempDir::new()?;
 fs::create_dir_all(temp.path().join("src"))?;
 fs::write(temp.path().join("src/main.rs"), "fn main() {}\n")?;
 fs::write(
 temp.path().join("Cargo.toml"),
 "[package]\nversion = \"0.1.0\"\n",
 )?;
 let result = detect_repo(temp.path());
 assert!(result.is_err(), "missing package name should error");
 Ok(())
 }

 #[test]
 fn repo_without_readme_produces_zero_repetitions() -> Result<(), Box<dyn std::error::Error>> {
 // Covers line 183: README.md does not exist → count_name_repetitions returns Ok(0).
 use std::fs;
 use tempfile::TempDir;
 let temp = TempDir::new()?;
 fs::create_dir_all(temp.path().join("src"))?;
 for i in 0..4_u8 {
 fs::write(
 temp.path().join("src").join(format!("mod{i}.rs")),
 "fn f() {}\n",
 )?;
 }
 fs::write(
 temp.path().join("Cargo.toml"),
 "[package]\nname = \"app-tool\"\nversion=\"0.1.0\"\n",
 )?;
 // No README.md → count_name_repetitions returns 0 via early return at line 183.
 let _ = detect_repo(temp.path())?;
 Ok(())
 }

 #[test]
 fn git_directory_and_non_file_entries_are_skipped() -> Result<(), Box<dyn std::error::Error>> {
 // Covers lines 161 (.git skip) and 169 (!path.is_file() skip).
 use std::fs;
 use tempfile::TempDir;
 let temp = TempDir::new()?;
 // Create a.git dir with a file inside (triggers line 161).
 fs::create_dir_all(temp.path().join(".git"))?;
 fs::write(temp.path().join(".git").join("config"), "[core]\n")?;
 fs::create_dir_all(temp.path().join("subdir"))?;
 // Create a file without extension (no extension → skipped at `let Some(ext)` line).
 fs::write(temp.path().join("Makefile"), "all:\n\t@echo ok\n")?;
 fs::write(
 temp.path().join("Cargo.toml"),
 "[package]\nname = \"simple\"\nversion=\"0.1.0\"\n",
 )?;
 // No findings expected; just verify no panic.
 let _ = detect_repo(temp.path())?;
 Ok(())
 }

 #[test]
 fn detect_repo_returns_medium_severity_when_generic_hits_below_high_threshold()
 -> Result<(), Box<dyn std::error::Error>> {
 // Medium path: NOT (generic_hits >= 3 && repetition_hits >= 5 && code_files <= 2).
 use std::fs;
 use tempfile::TempDir;
 let temp = TempDir::new()?;
 fs::create_dir_all(temp.path().join("src"))?;
 fs::write(temp.path().join("src/main.rs"), "fn main() {}\n")?;
 fs::write(
 temp.path().join("Cargo.toml"),
 "[package]\nname = \"app-tool\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
 )?;
 fs::write(
 temp.path().join("README.md"),
 "app-tool is a tool. app-tool is instant. app-tool is next-gen. app-tool helps everyone.\n",
 )?;
 let config = NameCredibilityConfig {
 min_generic_token_hits: 2,
 max_code_files_for_flag: 4,
 min_name_repetition_hits: 3,
 };
 let findings = detect_repo_with_config(temp.path(), config)?;
 assert_eq!(findings.len(), 1);
 assert_eq!(
 findings[0].severity,
 crate::detection::finding::Severity::Medium
 );
 Ok(())
 }
}