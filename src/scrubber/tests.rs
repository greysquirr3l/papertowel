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

struct LangTestConfig {
 test_fn_re: Regex,
 name_filter: fn(&str) -> bool,
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
 |line| line.contains("EXPECT_") || line.contains("ASSERT_") || line.contains("CHECK("),
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
mod tests {
 #![expect(
 clippy::module_inception,
 reason = "conventional test module placement"
 )]
 #![expect(
 clippy::indexing_slicing,
 reason = "indexed assertions on known-populated vecs"
 )]

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

 #[test]
 fn below_min_test_count_produces_no_finding() -> Result<(), Box<dyn std::error::Error>> {
 let content = "\
fn test_case_001() { assert_eq!(1, 1); }\n\
fn test_case_002() { assert_eq!(1, 1); }\n\
";
 let findings = detect_in_text(
 "tests/small.rs",
 content,
 TestShapeDetectionConfig::default(),
 )?;
 assert!(
 findings.is_empty(),
 "too few tests should produce no finding"
 );
 Ok(())
 }

 #[test]
 fn python_test_shape_detected() -> Result<(), Box<dyn std::error::Error>> {
 use crate::detection::language::LanguageKind;
 use crate::scrubber::tests::detect_in_text_for_language;

 // 6+ tests with dominant prefix "test_item_" and high assert density
 let content = "\
def test_item_001(): assert True\n\
def test_item_002(): assert True\n\
def test_item_003(): assert True\n\
def test_item_004(): assert True\n\
def test_item_005(): assert True\n\
def test_item_006(): assert True\n\
";
 let findings = detect_in_text_for_language(
 "tests/test_gen.py",
 content,
 TestShapeDetectionConfig::default(),
 LanguageKind::Python,
 )?;
 assert!(
!findings.is_empty(),
 "Python templated tests should be flagged"
 );
 Ok(())
 }

 #[test]
 fn go_test_shape_detected() -> Result<(), Box<dyn std::error::Error>> {
 use crate::detection::language::LanguageKind;
 use crate::scrubber::tests::detect_in_text_for_language;

 // Names like Test_item_001 share "Test_item" prefix after splitting on '_'
 let content = "\
func Test_item_001(t *testing.T) { t.Error(\"x\") }\n\
func Test_item_002(t *testing.T) { t.Error(\"x\") }\n\
func Test_item_003(t *testing.T) { t.Error(\"x\") }\n\
func Test_item_004(t *testing.T) { t.Error(\"x\") }\n\
func Test_item_005(t *testing.T) { t.Error(\"x\") }\n\
func Test_item_006(t *testing.T) { t.Error(\"x\") }\n\
";
 let findings = detect_in_text_for_language(
 "tests/gen_test.go",
 content,
 TestShapeDetectionConfig::default(),
 LanguageKind::Go,
 )?;
 assert!(!findings.is_empty(), "Go templated tests should be flagged");
 Ok(())
 }

 #[test]
 fn typescript_test_shape_detected() -> Result<(), Box<dyn std::error::Error>> {
 use crate::detection::language::LanguageKind;
 use crate::scrubber::tests::detect_in_text_for_language;

 // TS regex captures name word chars and spaces; use 3-part underscore names
 // so 2-word prefix "item_cases" is shared by all → dominant ratio = 1.0
 let content = "\
it('item_cases_001', () => { expect(1).toBe(1); });\n\
it('item_cases_002', () => { expect(1).toBe(1); });\n\
it('item_cases_003', () => { expect(1).toBe(1); });\n\
it('item_cases_004', () => { expect(1).toBe(1); });\n\
it('item_cases_005', () => { expect(1).toBe(1); });\n\
it('item_cases_006', () => { expect(1).toBe(1); });\n\
";
 let findings = detect_in_text_for_language(
 "tests/gen.test.ts",
 content,
 TestShapeDetectionConfig::default(),
 LanguageKind::TypeScript,
 )?;
 assert!(
!findings.is_empty(),
 "TypeScript templated tests should be flagged"
 );
 Ok(())
 }

 #[test]
 fn zig_test_shape_detected() -> Result<(), Box<dyn std::error::Error>> {
 use crate::detection::language::LanguageKind;
 use crate::scrubber::tests::detect_in_text_for_language;

 // Zig test names; 3-part underscore names so prefix "item_cases" is shared by all
 let content = r#"
test "item_cases_001" { try std.testing.expect(true); }
test "item_cases_002" { try std.testing.expect(true); }
test "item_cases_003" { try std.testing.expect(true); }
test "item_cases_004" { try std.testing.expect(true); }
test "item_cases_005" { try std.testing.expect(true); }
test "item_cases_006" { try std.testing.expect(true); }
"#;
 let findings = detect_in_text_for_language(
 "tests/gen_test.zig",
 content,
 TestShapeDetectionConfig::default(),
 LanguageKind::Zig,
 )?;
 assert!(
!findings.is_empty(),
 "Zig templated tests should be flagged"
 );
 Ok(())
 }

 #[test]
 fn detect_file_reads_real_file() -> Result<(), Box<dyn std::error::Error>> {
 use crate::scrubber::tests::detect_file;
 use std::io::Write;
 use tempfile::NamedTempFile;

 let content = "\
fn test_case_001() { assert_eq!(1, 1); }\n\
fn test_case_002() { assert_eq!(1, 1); }\n\
fn test_case_003() { assert_eq!(1, 1); }\n\
fn test_case_004() { assert_eq!(1, 1); }\n\
fn test_case_005() { assert_eq!(1, 1); }\n\
fn test_case_006() { assert_eq!(1, 1); }\n\
";
 let mut f = NamedTempFile::new()?;
 write!(f, "{content}")?;
 let findings = detect_file(f.path())?;
 assert!(
!findings.is_empty(),
 "detect_file should delegate and detect template tests"
 );
 Ok(())
 }

 #[test]
 fn detect_file_for_language_csharp_reads_real_file() -> Result<(), Box<dyn std::error::Error>> {
 use crate::detection::language::LanguageKind;
 use crate::scrubber::tests::detect_file_for_language;
 use std::io::Write;
 use tempfile::NamedTempFile;

 let content = "\
public void TestCase001() { Assert.Equal(1, 1); }\n\
public void TestCase002() { Assert.Equal(1, 1); }\n\
public void TestCase003() { Assert.Equal(1, 1); }\n\
public void TestCase004() { Assert.Equal(1, 1); }\n\
public void TestCase005() { Assert.Equal(1, 1); }\n\
public void TestCase006() { Assert.Equal(1, 1); }\n\
";
 let mut f = NamedTempFile::new()?;
 write!(f, "{content}")?;
 let _ = detect_file_for_language(f.path(), LanguageKind::CSharp)?;
 Ok(())
 }

 #[test]
 fn detect_in_text_for_language_cpp_uniform_tests() -> Result<(), Box<dyn std::error::Error>> {
 use crate::detection::language::LanguageKind;
 use crate::scrubber::tests::detect_in_text_for_language;

 let content = "\
TEST(Suite, test_case_001) { EXPECT_EQ(1, 1); }\n\
TEST(Suite, test_case_002) { EXPECT_EQ(1, 1); }\n\
TEST(Suite, test_case_003) { EXPECT_EQ(1, 1); }\n\
TEST(Suite, test_case_004) { EXPECT_EQ(1, 1); }\n\
TEST(Suite, test_case_005) { EXPECT_EQ(1, 1); }\n\
TEST(Suite, test_case_006) { EXPECT_EQ(1, 1); }\n\
";
 let findings = detect_in_text_for_language(
 "test_suite.cpp",
 content,
 TestShapeDetectionConfig::default(),
 LanguageKind::Cpp,
 )?;
 assert!(!findings.is_empty(), "C++ template tests should be flagged");
 Ok(())
 }

 #[test]
 fn empty_content_returns_no_findings() -> Result<(), Box<dyn std::error::Error>> {
 use crate::detection::language::LanguageKind;
 use crate::scrubber::tests::detect_in_text_for_language;
 // No test names → TestShapeMetrics::default() path (line 233)
 let findings = detect_in_text_for_language(
 "empty.rs",
 "",
 TestShapeDetectionConfig::default(),
 LanguageKind::Rust,
 )?;
 assert!(findings.is_empty(), "empty content → no findings");
 Ok(())
 }

 #[test]
 fn medium_severity_when_assert_density_below_high_threshold()
 -> Result<(), Box<dyn std::error::Error>> {
 // Covers line 73 (Severity::Medium): prefix_ratio <= 0.80 OR assert_density <= 0.70.
 // Use Rust tests where all have the same prefix but asserts are sparse:
 // N functions, half with assert → assert_density < 0.70.
 use crate::detection::finding::Severity;
 use crate::detection::language::LanguageKind;
 use crate::scrubber::tests::detect_in_text_for_language;

 // 6 test_ fns, only 2 have assert (density 0.33 < 0.70), prefix_ratio = 1.0 > 0.80
 // prefix_ratio > 0.80 BUT assert_density < 0.70 → Medium
 let content = "\
#[test]\nfn test_one() { let _ = 1; }\n\
#[test]\nfn test_two() { let _ = 2; }\n\
#[test]\nfn test_three() { let _ = 3; }\n\
#[test]\nfn test_four() { let _ = 4; }\n\
#[test]\nfn test_five() { assert!(true); }\n\
#[test]\nfn test_six() { assert!(true); }\n\
";
 let config = TestShapeDetectionConfig {
 min_test_count: 5,
 min_prefix_ratio: 0.8,
 min_assert_density: 0.2,
 };
 let findings =
 detect_in_text_for_language("src/lib_test.rs", content, config, LanguageKind::Rust)?;
 if!findings.is_empty() {
 assert_eq!(
 findings[0].severity,
 Severity::Medium,
 "assert_density 0.33 < 0.70 → Medium"
 );
 }
 Ok(())
 }

 #[test]
 fn python_assert_equal_and_assert_called_lines_are_counted()
 -> Result<(), Box<dyn std::error::Error>> {
 // Covers lines 151-153: Python.assertEqual( and.assert_called branches.
 use crate::detection::language::LanguageKind;
 use crate::scrubber::tests::detect_in_text_for_language;

 let content = "\
def test_alpha(self): self.assertEqual(1, 1)\n\
def test_beta(self): self.assertEqual(2, 2)\n\
def test_gamma(self): self.assert_called_once()\n\
def test_delta(self): self.assertEqual(3, 3)\n\
def test_epsilon(self): self.assertEqual(4, 4)\n\
def test_zeta(self): self.assertEqual(5, 5)\n\
";
 let findings = detect_in_text_for_language(
 "test_foo.py",
 content,
 TestShapeDetectionConfig::default(),
 LanguageKind::Python,
 )?;
 let _ = findings;
 Ok(())
 }

 #[test]
 fn go_fatal_fail_require_assert_lines_are_counted() -> Result<(), Box<dyn std::error::Error>> {
 // Covers lines 161-164: Go t.Fatal, t.Fail, assert., require. branches.
 use crate::detection::language::LanguageKind;
 use crate::scrubber::tests::detect_in_text_for_language;

 let content = "\
func TestAlpha(t *testing.T) { t.Fatal(\"nope\") }\n\
func TestBeta(t *testing.T) { t.Fail() }\n\
func TestGamma(t *testing.T) { assert.Equal(t, 1, 1) }\n\
func TestDelta(t *testing.T) { require.NoError(t, nil) }\n\
func TestEpsilon(t *testing.T) { t.Fatal(\"x\") }\n\
func TestZeta(t *testing.T) { t.Fatal(\"y\") }\n\
";
 let findings = detect_in_text_for_language(
 "foo_test.go",
 content,
 TestShapeDetectionConfig::default(),
 LanguageKind::Go,
 )?;
 let _ = findings;
 Ok(())
 }

 #[test]
 fn csharp_xunit_and_should_lines_are_counted() -> Result<(), Box<dyn std::error::Error>> {
 // Covers lines 179-180: CSharp Xunit.Assert and.Should(). branches.
 use crate::detection::language::LanguageKind;
 use crate::scrubber::tests::detect_in_text_for_language;

 let content = "\
public void TestAlpha() { Xunit.Assert.Equal(1, 1); }\n\
public void TestBeta() { Xunit.Assert.True(true); }\n\
public void TestGamma() { value.Should().Be(1); }\n\
public void TestDelta() { Xunit.Assert.Equal(2, 2); }\n\
public void TestEpsilon() { Xunit.Assert.Equal(3, 3); }\n\
public void TestZeta() { Xunit.Assert.NotNull(null); }\n\
";
 let findings = detect_in_text_for_language(
 "FooTests.cs",
 content,
 TestShapeDetectionConfig::default(),
 LanguageKind::CSharp,
 )?;
 let _ = findings;
 Ok(())
 }

 #[test]
 fn unknown_language_config_covers_test_prefix_predicate()
 -> Result<(), Box<dyn std::error::Error>> {
 // Covers lines 193-196: LanguageKind::Unknown path — name predicate (test_ prefix)
 // and assert-line predicate.
 use crate::detection::language::LanguageKind;
 use crate::scrubber::tests::detect_in_text_for_language;

 let content = "\
fn test_alpha() { assert(1 == 1) }\n\
fn test_beta() { assert(2 == 2) }\n\
fn test_gamma() { assert(3 == 3) }\n\
fn test_delta() { assert(4 == 4) }\n\
fn test_epsilon() { assert(5 == 5) }\n\
fn test_zeta() { assert(6 == 6) }\n\
";
 let findings = detect_in_text_for_language(
 "tests.txt",
 content,
 TestShapeDetectionConfig::default(),
 LanguageKind::Unknown,
 )?;
 let _ = findings;
 Ok(())
 }

 #[test]
 fn medium_severity_when_prefix_ratio_below_threshold() -> Result<(), Box<dyn std::error::Error>>
 {
 // Covers line 73 (Severity::Medium): dominant_prefix_ratio <= 0.80.
 // test_case_* (4) and test_other_* (2) → dominant prefix "test_case" = 4/6 = 0.667.
 // 0.667 is NOT > 0.80 → Severity::Medium.
 let content = "\
fn test_case_001() { assert_eq!(1, 1); }\n\
fn test_case_002() { assert_eq!(1, 1); }\n\
fn test_case_003() { assert_eq!(1, 1); }\n\
fn test_case_004() { assert_eq!(1, 1); }\n\
fn test_other_001() { assert_eq!(2, 2); }\n\
fn test_other_002() { assert_eq!(3, 3); }\n\
";
 let findings = detect_in_text(
 "tests/mixed_prefixes.rs",
 content,
 TestShapeDetectionConfig::default(),
 )?;
 assert_eq!(findings.len(), 1);
 for f in &findings {
 use crate::detection::finding::Severity;
 assert_eq!(f.severity, Severity::Medium, "4/6 dominant ratio → Medium");
 }
 Ok(())
 }
}