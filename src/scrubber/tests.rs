use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

use crate::detection::finding::{Finding, FindingCategory, LineRange, Severity};
use crate::detection::language::LanguageKind;
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
    detect_file_for_language(path, LanguageKind::Rust)
}

/// Language-aware variant of [`detect_file`].
pub fn detect_file_for_language(
    path: impl AsRef<Path>,
    lang: LanguageKind,
) -> Result<Vec<Finding>, PapertowelError> {
    let path = path.as_ref();
    let content =
        fs::read_to_string(path).map_err(|error| PapertowelError::io_with_path(path, error))?;
    detect_in_text_for_language(path, &content, TestShapeDetectionConfig::default(), lang)
}

pub fn detect_in_text(
    file_path: impl Into<PathBuf>,
    content: &str,
    config: TestShapeDetectionConfig,
) -> Result<Vec<Finding>, PapertowelError> {
    detect_in_text_for_language(file_path, content, config, LanguageKind::Rust)
}

/// Language-aware variant of [`detect_in_text`].
pub fn detect_in_text_for_language(
    file_path: impl Into<PathBuf>,
    content: &str,
    config: TestShapeDetectionConfig,
    lang: LanguageKind,
) -> Result<Vec<Finding>, PapertowelError> {
    let file_path = file_path.into();
    let metrics = analyze_test_shape_for_language(content, lang)?;

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

/// Language configuration for test-name and assertion detection.
struct LangTestConfig {
    /// Regex to extract the test function name (capture group 1 = name).
    test_fn_re: Regex,
    /// Prefix/suffix the test name must have to be counted (empty = all fns).
    name_filter: fn(&str) -> bool,
    /// Returns `true` when the trimmed line contains an assertion.
    assert_filter: fn(&str) -> bool,
}

impl LangTestConfig {
    fn new(
        pattern: &str,
        name_filter: fn(&str) -> bool,
        assert_filter: fn(&str) -> bool,
    ) -> Result<Self, PapertowelError> {
        let test_fn_re = Regex::new(pattern)
            .map_err(|e| PapertowelError::Validation(format!("invalid test regex: {e}")))?;
        Ok(Self {
            test_fn_re,
            name_filter,
            assert_filter,
        })
    }
}

fn make_lang_test_config(lang: LanguageKind) -> Result<LangTestConfig, PapertowelError> {
    match lang {
        LanguageKind::Rust => LangTestConfig::new(
            r"fn\s+([A-Za-z0-9_]+)",
            |name| name.starts_with("test_"),
            |line| {
                line.contains("assert_")
                    || line.starts_with("assert!(")
                    || line.starts_with("assert_eq!(")
            },
        ),
        LanguageKind::Python => LangTestConfig::new(
            r"def\s+(test_[A-Za-z0-9_]+)",
            |_| true, // regex already filters by prefix
            |line| {
                line.contains("assert ")
                    || line.contains(".assert_")
                    || line.contains(".assertEqual(")
                    || line.contains(".assert_called")
            },
        ),
        LanguageKind::Go => LangTestConfig::new(
            r"func\s+(Test[A-Za-z0-9_]+)",
            |_| true,
            |line| {
                line.contains("t.Error")
                    || line.contains("t.Fatal")
                    || line.contains("t.Fail")
                    || line.contains("assert.")
                    || line.contains("require.")
            },
        ),
        LanguageKind::TypeScript => LangTestConfig::new(
            r#"(?:it|test)\s*\(\s*['"]([A-Za-z0-9_ ]+)['"]"#,
            |_| true,
            |line| {
                line.contains("expect(") || line.contains(".toBe(") || line.contains(".toEqual(")
            },
        ),
        LanguageKind::CSharp => LangTestConfig::new(
            r"(?:public\s+)?(?:void|Task)\s+(\w+)\s*\(",
            |_| true,
            |line| {
                line.contains("Assert.")
                    || line.contains("Xunit.Assert")
                    || line.contains(".Should().")
            },
        ),
        LanguageKind::Zig => LangTestConfig::new(
            r#"test\s+"([^"]+)""#,
            |_| true,
            |line| line.contains("try std.testing.expect") || line.contains("testing.expect"),
        ),
        LanguageKind::Cpp => LangTestConfig::new(
            r"TEST(?:_F|_P)?\s*\(\s*\w+\s*,\s*(\w+)\s*\)",
            |_| true,
            |line| {
                line.contains("EXPECT_") || line.contains("ASSERT_") || line.contains("CHECK(")
            },
        ),
        LanguageKind::Unknown => LangTestConfig::new(
            r"fn\s+([A-Za-z0-9_]+)",
            |name| name.starts_with("test_"),
            |line| line.contains("assert"),
        ),
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "density ratios: bounded usize counts"
)]
fn analyze_test_shape_for_language(
    content: &str,
    lang: LanguageKind,
) -> Result<TestShapeMetrics, PapertowelError> {
    let cfg = make_lang_test_config(lang)?;
    let regex = &cfg.test_fn_re;

    let mut test_names = Vec::new();
    let mut assert_count = 0_usize;

    for line in content.lines() {
        let trimmed = line.trim();
        if (cfg.assert_filter)(trimmed) {
            assert_count += 1;
        }

        if let Some(caps) = regex.captures(trimmed) {
            let name = caps.get(1);
            if let Some(name) = name {
                let as_str = name.as_str();
                if (cfg.name_filter)(as_str) {
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
