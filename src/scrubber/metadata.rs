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

const BOILERPLATE_MARKERS: [&str; 6] = [
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
    pub min_boilerplate_hits: usize,
}

impl Default for MetadataDetectionConfig {
    fn default() -> Self {
        Self {
            min_present_files: 3,
            min_boilerplate_hits: 5,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataScanResult {
    pub present_files: Vec<String>,
    pub boilerplate_hits: usize,
}

pub fn detect_repo(repo_root: impl AsRef<Path>) -> Result<Vec<Finding>, PapertowelError> {
    detect_repo_with_config(repo_root, MetadataDetectionConfig::default())
}

pub fn detect_repo_with_config(
    repo_root: impl AsRef<Path>,
    config: MetadataDetectionConfig,
) -> Result<Vec<Finding>, PapertowelError> {
    let repo_root = repo_root.as_ref();
    let scan = scan_metadata(repo_root)?;

    if scan.present_files.len() < config.min_present_files
        || scan.boilerplate_hits < config.min_boilerplate_hits
    {
        return Ok(Vec::new());
    }

    let severity = if scan.present_files.len() >= 4 && scan.boilerplate_hits >= 8 {
        Severity::High
    } else {
        Severity::Medium
    };

    let confidence = ((scan.present_files.len() as f32 / METADATA_FILES.len() as f32) * 0.6
        + (scan.boilerplate_hits as f32 / 12.0) * 0.4)
        .min(1.0);

    let mut finding = Finding::new(
        "metadata.boilerplate.bundle",
        FindingCategory::Metadata,
        severity,
        confidence,
        repo_root.join("."),
        format!(
            "Detected metadata boilerplate bundle ({} files, {} phrase hits): {}",
            scan.present_files.len(),
            scan.boilerplate_hits,
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
    let mut boilerplate_hits = 0_usize;

    for file_name in METADATA_FILES {
        let path = repo_root.join(file_name);
        if !path.is_file() {
            continue;
        }

        present_files.push(file_name.to_owned());
        let content = fs::read_to_string(&path)
            .map_err(|error| PapertowelError::io_with_path(&path, error))?;
        let lowered = content.to_ascii_lowercase();
        boilerplate_hits += BOILERPLATE_MARKERS
            .iter()
            .filter(|marker| lowered.contains(**marker))
            .count();
    }

    Ok(MetadataScanResult {
        present_files,
        boilerplate_hits,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use crate::scrubber::metadata::{
        DETECTOR_NAME, MetadataDetectionConfig, detect_repo_with_config,
    };

    #[test]
    fn detector_name_is_stable() {
        assert_eq!(DETECTOR_NAME, "metadata");
    }

    #[test]
    fn metadata_detector_ignores_sparse_docs() {
        let temp = TempDir::new();
        assert!(temp.is_ok());
        let temp = match temp {
            Ok(temp) => temp,
            Err(error) => panic!("failed to create tempdir: {error}"),
        };

        let write = fs::write(
            temp.path().join("CONTRIBUTING.md"),
            "Repository-specific contribution workflow.",
        );
        assert!(write.is_ok());

        let findings = detect_repo_with_config(temp.path(), MetadataDetectionConfig::default());
        assert!(findings.is_ok());
        let findings = match findings {
            Ok(findings) => findings,
            Err(error) => panic!("unexpected metadata detector error: {error}"),
        };
        assert!(findings.is_empty());
    }

    #[test]
    fn metadata_detector_flags_policy_bundle() {
        let temp = TempDir::new();
        assert!(temp.is_ok());
        let temp = match temp {
            Ok(temp) => temp,
            Err(error) => panic!("failed to create tempdir: {error}"),
        };

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
            let write = fs::write(temp.path().join(name), content);
            assert!(write.is_ok());
        }

        let findings = detect_repo_with_config(temp.path(), MetadataDetectionConfig::default());
        assert!(findings.is_ok());
        let findings = match findings {
            Ok(findings) => findings,
            Err(error) => panic!("unexpected metadata detector error: {error}"),
        };

        assert_eq!(findings.len(), 1);
    }
}
