use std::fs;
use std::path::Path;

use crate::detection::finding::{Finding, FindingCategory, LineRange, Severity};
use crate::domain::errors::PapertowelError;

pub const DETECTOR_NAME: &str = "workflow";

const WORKFLOW_FILES: [&str; 7] = [
    ".github/workflows/ci.yml",
    ".github/workflows/release.yml",
    ".github/ISSUE_TEMPLATE/bug_report.md",
    ".github/ISSUE_TEMPLATE/feature_request.md",
    ".github/PULL_REQUEST_TEMPLATE.md",
    "CODEOWNERS",
    "dependabot.yml",
];

const WORKFLOW_MARKERS: [&str; 7] = [
    "welcome contributors",
    "thanks for taking the time",
    "automatically generated",
    "please fill out",
    "lint, test, and release",
    "continuous integration",
    "template",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowDetectionConfig {
    pub min_workflow_files: usize,
    pub min_marker_hits: usize,
}

impl Default for WorkflowDetectionConfig {
    fn default() -> Self {
        Self {
            min_workflow_files: 3,
            min_marker_hits: 3,
        }
    }
}

pub fn detect_repo(repo_root: impl AsRef<Path>) -> Result<Vec<Finding>, PapertowelError> {
    detect_repo_with_config(repo_root, WorkflowDetectionConfig::default())
}

#[expect(
    clippy::cast_precision_loss,
    reason = "confidence score: bounded usize counts"
)]
pub fn detect_repo_with_config(
    repo_root: impl AsRef<Path>,
    config: WorkflowDetectionConfig,
) -> Result<Vec<Finding>, PapertowelError> {
    let repo_root = repo_root.as_ref();

    let mut present_files = Vec::new();
    let mut marker_hits = 0_usize;

    for file in WORKFLOW_FILES {
        let path = repo_root.join(file);
        if !path.is_file() {
            continue;
        }

        present_files.push(file.to_owned());
        let content = fs::read_to_string(&path)
            .map_err(|error| PapertowelError::io_with_path(&path, error))?;
        let lowered = content.to_ascii_lowercase();
        marker_hits += WORKFLOW_MARKERS
            .iter()
            .filter(|marker| lowered.contains(**marker))
            .count();
    }

    if present_files.len() < config.min_workflow_files || marker_hits < config.min_marker_hits {
        return Ok(Vec::new());
    }

    let severity = if present_files.len() >= 5 && marker_hits >= 5 {
        Severity::High
    } else {
        Severity::Medium
    };
    let confidence = (present_files.len() as f32 / WORKFLOW_FILES.len() as f32)
        .mul_add(0.7, (marker_hits as f32 / 10.0) * 0.3)
        .min(1.0);

    let mut finding = Finding::new(
        "workflow.artifact.bundle",
        FindingCategory::Workflow,
        severity,
        confidence,
        repo_root.join(".github"),
        format!(
            "Detected polished workflow/template burst ({} files, {} marker hits): {}",
            present_files.len(),
            marker_hits,
            present_files.join(", ")
        ),
    )?;
    finding.line_range = Some(LineRange::new(1, 1)?);
    finding.suggestion = Some(
		"Keep only workflow and template files that are actively used by the repository's real process."
			.to_owned(),
	);

    Ok(vec![finding])
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::indexing_slicing,
        reason = "indexed assertions on known-populated vecs"
    )]

    use std::fs;

    use tempfile::TempDir;

    use crate::scrubber::workflow::{
        DETECTOR_NAME, WorkflowDetectionConfig, detect_repo, detect_repo_with_config,
    };

    #[test]
    fn detector_name_is_stable() {
        assert_eq!(DETECTOR_NAME, "workflow");
    }

    #[test]
    fn workflow_detector_ignores_small_real_setup() -> Result<(), Box<dyn std::error::Error>> {
        let temp = TempDir::new()?;

        let workflows = temp.path().join(".github/workflows");
        fs::create_dir_all(&workflows)?;
        fs::write(workflows.join("ci.yml"), "name: ci\n")?;

        let findings = detect_repo_with_config(temp.path(), WorkflowDetectionConfig::default())?;
        assert!(findings.is_empty());
        Ok(())
    }

    #[test]
    fn workflow_detector_flags_template_burst() -> Result<(), Box<dyn std::error::Error>> {
        let temp = TempDir::new()?;

        let files = [
            (
                ".github/workflows/ci.yml",
                "name: continuous integration\n# lint, test, and release\n",
            ),
            (
                ".github/workflows/release.yml",
                "name: release\n# automatically generated template\n",
            ),
            (
                ".github/PULL_REQUEST_TEMPLATE.md",
                "Thanks for taking the time. Please fill out this template.\n",
            ),
        ];

        for (file, content) in files {
            let absolute = temp.path().join(file);
            if let Some(parent) = absolute.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(absolute, content)?;
        }

        let findings = detect_repo_with_config(temp.path(), WorkflowDetectionConfig::default())?;
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
    fn workflow_detector_produces_high_severity_for_large_burst()
    -> Result<(), Box<dyn std::error::Error>> {
        // Covers Severity::High branch (line 82): present_files >= 5 AND marker_hits >= 5.
        let temp = TempDir::new()?;

        // Create 5 of the 7 workflow files, each containing multiple marker phrases.
        let files = [
            (
                ".github/workflows/ci.yml",
                "name: continuous integration\n# lint, test, and release\nautomatically generated template\n",
            ),
            (
                ".github/workflows/release.yml",
                "name: release\nthanks for taking the time\nplease fill out\n",
            ),
            (
                ".github/ISSUE_TEMPLATE/bug_report.md",
                "# Bug Report\nthanks for taking the time\nautomatically generated\n",
            ),
            (
                ".github/ISSUE_TEMPLATE/feature_request.md",
                "# Feature Request\nwelcome contributors\nplease fill out this template\n",
            ),
            (
                ".github/PULL_REQUEST_TEMPLATE.md",
                "Thanks for taking the time. Please fill out this template.\nwelcome contributors\n",
            ),
        ];

        for (file, content) in files {
            let absolute = temp.path().join(file);
            if let Some(parent) = absolute.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(absolute, content)?;
        }

        let findings = detect_repo_with_config(
            temp.path(),
            WorkflowDetectionConfig {
                min_workflow_files: 5,
                min_marker_hits: 5,
            },
        )?;
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].severity,
            crate::detection::finding::Severity::High
        );
        Ok(())
    }

    #[test]
    fn detect_repo_returns_medium_severity_when_below_high_threshold()
    -> Result<(), Box<dyn std::error::Error>> {
        // Covers lines 97-99 (format! expansion) via Medium severity path.
        // present_files.len() < 5 OR marker_hits < 5 → Severity::Medium.
        // Use 3 files with enough markers to pass min_workflow_files=3,min_marker_hits=1
        // but still below the High threshold (5 files AND 5 hits).
        use std::fs;
        use tempfile::TempDir;
        let temp = TempDir::new()?;
        let files = [
            (
                ".github/workflows/ci.yml",
                "name: continuous integration\nautomatically generated template\n",
            ),
            (
                ".github/PULL_REQUEST_TEMPLATE.md",
                "Thanks for taking the time. Please fill out this template.\n",
            ),
            (
                ".github/ISSUE_TEMPLATE/bug_report.md",
                "# Bug Report\nwelcome contributors\n",
            ),
        ];
        for (file, content) in &files {
            let absolute = temp.path().join(file);
            if let Some(parent) = absolute.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(absolute, content)?;
        }
        let findings = detect_repo_with_config(
            temp.path(),
            WorkflowDetectionConfig {
                min_workflow_files: 3,
                min_marker_hits: 1,
            },
        )?;
        // Should produce exactly one finding at Medium severity.
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].severity,
            crate::detection::finding::Severity::Medium
        );
        Ok(())
    }
}
