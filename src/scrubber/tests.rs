use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

use crate::detection::finding::{Finding, FindingCategory, LineRange, Severity};
use crate::domain::errors::PapertowelError;

pub const DETECTOR_NAME: &str = "tests";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TestShapeDetectionConfig {
    pub min_test_count: usize,
    pub min_prefix_ratio: f32,
    pub min_assert_density: f32,
}

impl Default for TestShapeDetectionConfig {
    fn default() -> Self {
        Self {
            min_test_count: 6,
            min_prefix_ratio: 0.65,
            min_assert_density: 0.5,
        }
    }
}

pub fn detect_file(path: impl AsRef<Path>) -> Result<Vec<Finding>, PapertowelError> {
    let path = path.as_ref();
    let content =
        fs::read_to_string(path).map_err(|error| PapertowelError::io_with_path(path, error))?;
    detect_in_text(path, &content, TestShapeDetectionConfig::default())
}

pub fn detect_in_text(
    file_path: impl Into<PathBuf>,
    content: &str,
    config: TestShapeDetectionConfig,
) -> Result<Vec<Finding>, PapertowelError> {
    let file_path = file_path.into();
    let metrics = analyze_test_shape(content)?;

    if metrics.test_count < config.min_test_count
        || metrics.dominant_prefix_ratio < config.min_prefix_ratio
        || metrics.assert_density < config.min_assert_density
    {
        return Ok(Vec::new());
    }

    let severity = if metrics.dominant_prefix_ratio > 0.80 && metrics.assert_density > 0.70 {
        Severity::High
    } else {
        Severity::Medium
    };

    let confidence = metrics
        .dominant_prefix_ratio
        .mul_add(0.6, metrics.assert_density * 0.4)
        .min(1.0);
    let line_count = content.lines().count().max(1);

    let mut finding = Finding::new(
        "tests.symmetric.shape",
        FindingCategory::TestPattern,
        severity,
        confidence,
        file_path,
        format!(
            "Detected suspicious test symmetry ({} tests, dominant name prefix ratio {:.2}, assert density {:.2}).",
            metrics.test_count, metrics.dominant_prefix_ratio, metrics.assert_density
        ),
    )?;
    finding.line_range = Some(LineRange::new(1, line_count)?);
    finding.suggestion = Some(
		"Introduce varied test names and scenarios that reflect real edge cases instead of template-generated symmetry."
			.to_owned(),
	);

    Ok(vec![finding])
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct TestShapeMetrics {
    test_count: usize,
    dominant_prefix_ratio: f32,
    assert_density: f32,
}

#[expect(
    clippy::cast_precision_loss,
    reason = "density ratios: bounded usize counts"
)]
fn analyze_test_shape(content: &str) -> Result<TestShapeMetrics, PapertowelError> {
    let regex = Regex::new(r"fn\s+([A-Za-z0-9_]+)")
        .map_err(|error| PapertowelError::Validation(format!("invalid test regex: {error}")))?;

    let mut test_names = Vec::new();
    let mut assert_count = 0_usize;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.contains("assert_")
            || trimmed.starts_with("assert!(")
            || trimmed.starts_with("assert_eq!(")
        {
            assert_count += 1;
        }

        if let Some(caps) = regex.captures(trimmed) {
            let name = caps.get(1);
            if let Some(name) = name {
                let as_str = name.as_str();
                if as_str.starts_with("test_") {
                    test_names.push(as_str.to_owned());
                }
            }
        }
    }

    if test_names.is_empty() {
        return Ok(TestShapeMetrics::default());
    }

    let mut prefix_counts: HashMap<String, usize> = HashMap::new();
    for name in &test_names {
        let prefix = name.split('_').take(2).collect::<Vec<_>>().join("_");
        let next = prefix_counts.get(&prefix).copied().unwrap_or(0_usize) + 1;
        prefix_counts.insert(prefix, next);
    }

    let largest_prefix = prefix_counts.values().copied().max().unwrap_or(0_usize);
    let dominant_prefix_ratio = largest_prefix as f32 / test_names.len() as f32;
    let assert_density = assert_count as f32 / test_names.len() as f32;

    Ok(TestShapeMetrics {
        test_count: test_names.len(),
        dominant_prefix_ratio,
        assert_density,
    })
}

#[cfg(test)]
#[expect(
    clippy::module_inception,
    reason = "conventional test module placement"
)]
mod tests {
    use crate::scrubber::tests::{DETECTOR_NAME, TestShapeDetectionConfig, detect_in_text};

    #[test]
    fn detector_name_is_stable() {
        assert_eq!(DETECTOR_NAME, "tests");
    }

    #[test]
    fn test_shape_detector_ignores_varied_tests() -> Result<(), Box<dyn std::error::Error>> {
        let content = "\
fn test_parse_valid() { assert!(true); }\n\
fn test_parse_invalid() { assert!(true); }\n\
fn test_roundtrip_csv() { assert!(true); }\n\
fn test_roundtrip_json() { assert!(true); }\n\
";
        let findings =
            detect_in_text("tests/mod.rs", content, TestShapeDetectionConfig::default())?;
        assert!(findings.is_empty());
        Ok(())
    }

    #[test]
    fn test_shape_detector_flags_repeated_template_style() -> Result<(), Box<dyn std::error::Error>>
    {
        let content = "\
fn test_case_001() { assert_eq!(1, 1); }\n\
fn test_case_002() { assert_eq!(1, 1); }\n\
fn test_case_003() { assert_eq!(1, 1); }\n\
fn test_case_004() { assert_eq!(1, 1); }\n\
fn test_case_005() { assert_eq!(1, 1); }\n\
fn test_case_006() { assert_eq!(1, 1); }\n\
";

        let findings = detect_in_text(
            "tests/generated.rs",
            content,
            TestShapeDetectionConfig::default(),
        )?;
        assert_eq!(findings.len(), 1);
        Ok(())
    }
}
