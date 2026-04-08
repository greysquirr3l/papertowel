use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::detection::finding::{Finding, FindingCategory, LineRange, Severity};
use crate::domain::errors::PapertowelError;

pub const DETECTOR_NAME: &str = "structure";

/// Pattern that matches the start of a Rust function definition.
static FN_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?(?:async\s+)?fn\s+\w+")
        .expect("FN_PATTERN is a valid regex")
});

/// Pattern for `/// ` doc comments preceding function definitions.
static DOCSTRING_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*///").expect("DOCSTRING_PATTERN is a valid regex"));

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StructureDetectionConfig {
    /// Minimum number of functions required before analysis is attempted.
    pub min_function_count: usize,
    /// Coefficient of variation threshold: CV below this value indicates
    /// suspiciously uniform function lengths.
    pub max_cv_uniform: f32,
    /// Fraction of functions that must have docstrings to trigger the
    /// docstring-uniformity signal.
    pub min_docstring_coverage: f32,
    /// Fraction of functions that must be `pub fn` to raise the all-public
    /// signal (a secondary corroborating indicator).
    pub min_all_pub_fraction: f32,
}

impl Default for StructureDetectionConfig {
    fn default() -> Self {
        Self {
            min_function_count: 5,
            max_cv_uniform: 0.25,
            min_docstring_coverage: 0.88,
            min_all_pub_fraction: 0.93,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructureMetrics {
    pub function_count: usize,
    /// Coefficient of variation of function body lengths (0.0 = identical).
    pub length_cv: f32,
    /// Fraction of detected functions preceded by a `///` docstring.
    pub docstring_coverage: f32,
    /// Fraction of functions declared as `pub fn` (vs `fn`).
    pub pub_fraction: f32,
}

/// Per-function measurement collected during a single-pass parse.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FunctionMeasure {
    /// Inclusive range of source lines making up this function.
    line_range: (usize, usize),
    has_docstring: bool,
    is_pub: bool,
}

impl FunctionMeasure {
    fn body_lines(&self) -> usize {
        self.line_range.1.saturating_sub(self.line_range.0) + 1
    }
}

// ─── Public API ──────────────────────────────────────────────────────────────

pub fn detect_file(path: impl AsRef<Path>) -> Result<Vec<Finding>, PapertowelError> {
    let path = path.as_ref();
    let content =
        fs::read_to_string(path).map_err(|error| PapertowelError::io_with_path(path, error))?;
    detect_in_text(path, &content, StructureDetectionConfig::default())
}

pub fn detect_in_text(
    file_path: impl Into<PathBuf>,
    content: &str,
    config: StructureDetectionConfig,
) -> Result<Vec<Finding>, PapertowelError> {
    let file_path = file_path.into();
    let metrics = analyze_structure(content)?;

    if metrics.function_count < config.min_function_count {
        return Ok(Vec::new());
    }

    // Require at least two corroborating signals before emitting a finding.
    let cv_uniform = metrics.length_cv < config.max_cv_uniform;
    let docs_uniform = metrics.docstring_coverage >= config.min_docstring_coverage;
    let all_pub = metrics.pub_fraction >= config.min_all_pub_fraction;

    let signal_count = usize::from(cv_uniform) + usize::from(docs_uniform) + usize::from(all_pub);

    if signal_count < 2 {
        return Ok(Vec::new());
    }

    let severity = if signal_count == 3 || (cv_uniform && metrics.length_cv < 0.10) {
        Severity::High
    } else {
        Severity::Medium
    };

    let confidence =
        (1.0_f32 - metrics.length_cv).mul_add(0.5, metrics.docstring_coverage * 0.3) * 0.9;

    let evidence = format!(
        "function count: {}, length CV: {:.2}, docstring coverage: {:.0}%, pub fraction: {:.0}%",
        metrics.function_count,
        metrics.length_cv,
        metrics.docstring_coverage * 100.0,
        metrics.pub_fraction * 100.0,
    );

    let line_count = content.lines().count().max(1);
    let mut finding = Finding::new(
        "structure.uniform",
        FindingCategory::Structure,
        severity,
        confidence.clamp(0.0, 1.0),
        file_path,
        evidence,
    )?;
    finding.line_range = Some(LineRange::new(1, line_count)?);
    finding.suggestion = Some(
        "Vary function lengths, docstring styles, and visibility to reduce structural uniformity."
            .to_owned(),
    );

    Ok(vec![finding])
}

pub fn analyze_structure(content: &str) -> Result<StructureMetrics, PapertowelError> {
    let measures = extract_function_measures(content);

    if measures.is_empty() {
        return Ok(StructureMetrics {
            function_count: 0,
            length_cv: 0.0,
            docstring_coverage: 0.0,
            pub_fraction: 0.0,
        });
    }

    let lengths: Vec<f64> = measures.iter().map(|m| m.body_lines() as f64).collect();

    let length_cv = coefficient_of_variation(&lengths);
    let function_count = measures.len();

    let with_docs = measures.iter().filter(|m| m.has_docstring).count();
    let pub_count = measures.iter().filter(|m| m.is_pub).count();

    #[expect(
        clippy::cast_precision_loss,
        reason = "bounded counts: no meaningful precision loss"
    )]
    Ok(StructureMetrics {
        function_count,
        length_cv: length_cv as f32,
        docstring_coverage: with_docs as f32 / function_count as f32,
        pub_fraction: pub_count as f32 / function_count as f32,
    })
}

// ─── Internal helpers ────────────────────────────────────────────────────────

/// Walk source lines and extract per-function measurements using a simple
/// brace-depth counter.  Results are approximate (strings and comments
/// containing braces are not fully handled) but accurate enough for
/// structural heuristics.
fn extract_function_measures(content: &str) -> Vec<FunctionMeasure> {
    let lines: Vec<&str> = content.lines().collect();
    let mut measures = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        if !FN_PATTERN.is_match(line) {
            i += 1;
            continue;
        }

        // Determine whether the function has a preceding docstring.
        let has_docstring = i > 0
            && lines[..i]
                .iter()
                .rev()
                .take_while(|l| {
                    l.trim().starts_with("///") || l.trim().starts_with('#') || l.trim().is_empty()
                })
                .any(|l| DOCSTRING_PATTERN.is_match(l));

        let is_pub = line.trim_start().starts_with("pub");

        // Track brace depth to find the end of this function.
        let start = i;
        let mut depth: i32 = 0;
        let mut found_open = false;

        let end = 'outer: {
            while i < lines.len() {
                for ch in lines[i].chars() {
                    match ch {
                        '{' => {
                            depth += 1;
                            found_open = true;
                        }
                        '}' => {
                            depth -= 1;
                            if found_open && depth == 0 {
                                break 'outer i;
                            }
                        }
                        _ => {}
                    }
                }
                i += 1;
            }
            // If we never closed, use the last line we reached.
            i.saturating_sub(1)
        };

        measures.push(FunctionMeasure {
            line_range: (start, end),
            has_docstring,
            is_pub,
        });

        i += 1;
    }

    measures
}

/// Compute the coefficient of variation (std_dev / mean) for a slice of
/// non-negative samples.  Returns 0.0 for empty or single-element slices.
fn coefficient_of_variation(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }

    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;

    if mean == 0.0 {
        return 0.0;
    }

    let variance = values.iter().map(|&v| (v - mean).powi(2)).sum::<f64>() / n;
    variance.sqrt() / mean
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{StructureDetectionConfig, StructureMetrics, analyze_structure, detect_in_text};

    const UNIFORM_FUNCTIONS: &str = r#"
/// Returns the foo value.
pub fn get_foo() -> u32 {
    let x = 42;
    let y = x + 1;
    y
}

/// Returns the bar value.
pub fn get_bar() -> u32 {
    let x = 99;
    let y = x + 1;
    y
}

/// Returns the baz value.
pub fn get_baz() -> u32 {
    let x = 10;
    let y = x + 1;
    y
}

/// Returns the qux value.
pub fn get_qux() -> u32 {
    let x = 55;
    let y = x + 1;
    y
}

/// Returns the quux value.
pub fn get_quux() -> u32 {
    let x = 77;
    let y = x + 1;
    y
}
"#;

    const VARIED_FUNCTIONS: &str = r#"
fn init() {
    setup_logging();
}

fn process(items: &[Item]) -> Vec<Output> {
    items
        .iter()
        .filter(|i| i.valid)
        .map(|i| i.transform())
        .collect()
}

fn shutdown(config: &Config, handle: &Handle) -> Result<(), Error> {
    handle.flush()?;
    config.save()?;
    tracing::info!("shutdown complete");
    Ok(())
}

fn debug_dump(ctx: &Context) {
    for (k, v) in &ctx.map {
        println!("{k}: {v}");
    }
}

fn long_computation(a: u64, b: u64, c: u64) -> u64 {
    let step1 = a.wrapping_mul(b);
    let step2 = step1.wrapping_add(c);
    let step3 = step2.rotate_left(13);
    let step4 = step3 ^ a;
    let step5 = step4.wrapping_sub(1);
    step5
}
"#;

    #[test]
    fn uniform_functions_detected() {
        let findings = detect_in_text(
            PathBuf::from("src/lib.rs"),
            UNIFORM_FUNCTIONS,
            StructureDetectionConfig::default(),
        )
        .expect("detect_in_text");
        assert!(
            !findings.is_empty(),
            "uniform functions should trigger a finding"
        );
    }

    #[test]
    fn varied_functions_not_detected() {
        let findings = detect_in_text(
            PathBuf::from("src/lib.rs"),
            VARIED_FUNCTIONS,
            StructureDetectionConfig::default(),
        )
        .expect("detect_in_text");
        assert!(
            findings.is_empty(),
            "naturally varied code should not be flagged: {findings:?}"
        );
    }

    #[test]
    fn below_min_function_count_skips_analysis() {
        let content = r#"
pub fn single() -> u32 {
    42
}
"#;
        let findings = detect_in_text(
            PathBuf::from("src/lib.rs"),
            content,
            StructureDetectionConfig::default(),
        )
        .expect("detect_in_text");
        assert!(findings.is_empty(), "too few functions — should be skipped");
    }

    #[test]
    fn analyze_structure_returns_zero_metrics_for_empty() {
        let metrics = analyze_structure("").expect("analyze");
        assert_eq!(
            metrics,
            StructureMetrics {
                function_count: 0,
                length_cv: 0.0,
                docstring_coverage: 0.0,
                pub_fraction: 0.0,
            }
        );
    }

    #[test]
    fn low_cv_detected_for_identical_length_functions() {
        let metrics = analyze_structure(UNIFORM_FUNCTIONS).expect("analyze");
        assert!(metrics.function_count >= 5);
        assert!(
            metrics.length_cv < 0.20,
            "identical-length functions should have CV < 0.20, got {}",
            metrics.length_cv
        );
        assert!(
            metrics.docstring_coverage > 0.80,
            "all functions have docstrings"
        );
    }
}
