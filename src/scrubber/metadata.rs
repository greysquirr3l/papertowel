use std::fs;
use std::path::Path;

use crate::detection::finding::{Finding, FindingCategory, LineRange, Severity};
use crate::domain::errors::PapertowelError;

pub const DETECTOR_NAME: &str = "metadata";

const METADATA_FILES: [&str; 5] = [
 "CONTRIBUTING.md",
 "CODE_OF_CONDUCT.md",
 "SECURITY.md",
 "SUPPORT.md",
 "GOVERNANCE.md",
];

const SCAFFOLD_MARKERS: [&str; 6] = [
 "all contributors are expected",
 "by participating in this project",
 "security policy",
 "report a vulnerability",
 "code of conduct",
 "unless otherwise noted",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetadataDetectionConfig {
 pub min_present_files: usize,
 pub min_scaffold_hits: usize,
}

impl Default for MetadataDetectionConfig {
 fn default() -> Self {
 Self {
 min_present_files: 3,
 min_scaffold_hits: 5,
 }
 }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataScanResult {
 pub present_files: Vec<String>,
 pub scaffold_hits: usize,
}

pub fn detect_repo(repo_root: impl AsRef<Path>) -> Result<Vec<Finding>, PapertowelError> {
 detect_repo_with_config(repo_root, MetadataDetectionConfig::default())
}

#[expect(
 clippy::cast_precision_loss,
 reason = "confidence score: bounded usize counts"
)]
pub fn detect_repo_with_config(
 repo_root: impl AsRef<Path>,
 config: MetadataDetectionConfig,
) -> Result<Vec<Finding>, PapertowelError> {
 let repo_root = repo_root.as_ref();
 let scan = scan_metadata(repo_root)?;

 if scan.present_files.len() < config.min_present_files
 || scan.scaffold_hits < config.min_scaffold_hits
 {
 return Ok(Vec::new());
 }

 let severity = if scan.present_files.len() >= 4 && scan.scaffold_hits >= 8 {
 Severity::High
 } else {
 Severity::Medium
 };

 let confidence = (scan.present_files.len() as f32 / METADATA_FILES.len() as f32)
.mul_add(0.6, (scan.scaffold_hits as f32 / 12.0) * 0.4)
.min(1.0);

 let mut finding = Finding::new(
 "metadata.scaffold.bundle",
 FindingCategory::Metadata,
 severity,
 confidence,
 repo_root.join("."),
 format!(
 "Detected metadata scaffold bundle ({} files, {} phrase hits): {}",
 scan.present_files.len(),
 scan.scaffold_hits,
 scan.present_files.join(", ")
 ),
 )?;
 finding.line_range = Some(LineRange::new(1, 1)?);
 finding.suggestion = Some(
		"Keep only metadata docs that match the project's actual governance and support model; remove generic policy bundles."
.to_owned(),
	);

 Ok(vec![finding])
}

fn scan_metadata(repo_root: &Path) -> Result<MetadataScanResult, PapertowelError> {
 let mut present_files = Vec::new();
 let mut scaffold_hits = 0_usize;

 for file_name in METADATA_FILES {
 let path = repo_root.join(file_name);
 if!path.is_file() {
 continue;
 }

 present_files.push(file_name.to_owned());
 let content = fs::read_to_string(&path)
.map_err(|error| PapertowelError::io_with_path(&path, error))?;
 let lowered = content.to_ascii_lowercase();
 scaffold_hits += SCAFFOLD_MARKERS
.iter()
.filter(|marker| lowered.contains(**marker))
.count();
 }

 Ok(MetadataScanResult {
 present_files,
 scaffold_hits,
 })
}

#[cfg(test)]
mod tests {
 #![expect(
 clippy::indexing_slicing,
 reason = "indexed assertions on known-populated vecs"
 )]

 use std::fs;

 use tempfile::TempDir;

 use crate::scrubber::metadata::{
 DETECTOR_NAME, MetadataDetectionConfig, detect_repo, detect_repo_with_config,
 };

 #[test]
 fn detector_name_is_stable() {
 assert_eq!(DETECTOR_NAME, "metadata");
 }

 #[test]
 fn metadata_detector_ignores_sparse_docs() -> Result<(), Box<dyn std::error::Error>> {
 let temp = TempDir::new()?;

 fs::write(
 temp.path().join("CONTRIBUTING.md"),
 "Repository-specific contribution workflow.",
 )?;

 let findings = detect_repo_with_config(temp.path(), MetadataDetectionConfig::default())?;
 assert!(findings.is_empty());
 Ok(())
 }

 #[test]
 fn metadata_detector_flags_policy_bundle() -> Result<(), Box<dyn std::error::Error>> {
 let temp = TempDir::new()?;

 let files = [
 (
 "CONTRIBUTING.md",
 "All contributors are expected to follow the code of conduct.",
 ),
 (
 "CODE_OF_CONDUCT.md",
 "By participating in this project, all contributors are expected to comply.",
 ),
 (
 "SECURITY.md",
 "Security policy: report a vulnerability unless otherwise noted.",
 ),
 ];

 for (name, content) in files {
 fs::write(temp.path().join(name), content)?;
 }

 let findings = detect_repo_with_config(temp.path(), MetadataDetectionConfig::default())?;
 assert_eq!(findings.len(), 1);
 Ok(())
 }

 #[test]
 fn detect_repo_delegates_to_with_config() -> Result<(), Box<dyn std::error::Error>> {
 use tempfile::TempDir;
 let temp = TempDir::new()?;
 let findings = detect_repo(temp.path())?;
 let _ = findings;
 Ok(())
 }

 #[test]
 fn high_severity_when_four_files_and_eight_hits() -> Result<(), Box<dyn std::error::Error>> {
 // Covers Severity::High path (line 69): present_files >= 4 AND scaffold_hits >= 8.
 use std::fs;
 use tempfile::TempDir;
 let temp = TempDir::new()?;

 // Create 4 metadata files, each stuffed with multiple scaffold markers.
 let heavy_content = "All contributors are expected to follow the code of conduct.\nBy participating in this project you agree.\nSecurity policy: report a vulnerability unless otherwise noted.\nCode of conduct applies to all spaces.\n";
 for name in [
 "CONTRIBUTING.md",
 "CODE_OF_CONDUCT.md",
 "SECURITY.md",
 "SUPPORT.md",
 ] {
 fs::write(temp.path().join(name), heavy_content)?;
 }

 let findings = detect_repo_with_config(
 temp.path(),
 MetadataDetectionConfig {
 min_present_files: 4,
 min_scaffold_hits: 8,
 },
 )?;
 assert_eq!(findings.len(), 1);
 assert_eq!(
 findings[0].severity,
 crate::detection::finding::Severity::High
 );
 Ok(())
 }
}