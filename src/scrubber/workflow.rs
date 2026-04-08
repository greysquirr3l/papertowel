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
		let content = fs::read_to_string(&path).map_err(|error| PapertowelError::io_with_path(&path, error))?;
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
	let confidence = ((present_files.len() as f32 / WORKFLOW_FILES.len() as f32) * 0.7
		+ (marker_hits as f32 / 10.0) * 0.3)
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
	use std::fs;

	use tempfile::TempDir;

	use crate::scrubber::workflow::{
		DETECTOR_NAME, WorkflowDetectionConfig, detect_repo_with_config,
	};

	#[test]
	fn detector_name_is_stable() {
		assert_eq!(DETECTOR_NAME, "workflow");
	}

	#[test]
	fn workflow_detector_ignores_small_real_setup() {
		let temp = TempDir::new();
		assert!(temp.is_ok());
		let temp = match temp {
			Ok(temp) => temp,
			Err(error) => panic!("failed to create tempdir: {error}"),
		};

		let workflows = temp.path().join(".github/workflows");
		let created = fs::create_dir_all(&workflows);
		assert!(created.is_ok());
		let written = fs::write(workflows.join("ci.yml"), "name: ci\n");
		assert!(written.is_ok());

		let findings = detect_repo_with_config(temp.path(), WorkflowDetectionConfig::default());
		assert!(findings.is_ok());
		let findings = match findings {
			Ok(findings) => findings,
			Err(error) => panic!("unexpected workflow detector error: {error}"),
		};
		assert!(findings.is_empty());
	}

	#[test]
	fn workflow_detector_flags_template_burst() {
		let temp = TempDir::new();
		assert!(temp.is_ok());
		let temp = match temp {
			Ok(temp) => temp,
			Err(error) => panic!("failed to create tempdir: {error}"),
		};

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
				let created = fs::create_dir_all(parent);
				assert!(created.is_ok());
			}
			let write = fs::write(absolute, content);
			assert!(write.is_ok());
		}

		let findings = detect_repo_with_config(temp.path(), WorkflowDetectionConfig::default());
		assert!(findings.is_ok());
		let findings = match findings {
			Ok(findings) => findings,
			Err(error) => panic!("unexpected workflow detector error: {error}"),
		};
		assert_eq!(findings.len(), 1);
	}
}