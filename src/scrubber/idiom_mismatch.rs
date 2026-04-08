use std::fs;
use std::path::{Path, PathBuf};

use crate::detection::finding::{Finding, FindingCategory, LineRange, Severity};
use crate::domain::errors::PapertowelError;

pub const DETECTOR_NAME: &str = "idiom_mismatch";

const FOREIGN_IDIOM_MARKERS: [&str; 14] = [
    "console.log(",
    "package main",
    "fmt.println(",
    "public static void main",
    "system.out.println(",
    "try:\n",
    "except ",
    "def __init__",
    "class ",
    "self.",
    "await asyncio",
    "npm install",
    "pip install",
    "golang",
];

const RUST_IDIOM_MARKERS: [&str; 10] = [
    "result<",
    "option<",
    "if let",
    "match ",
    "impl ",
    "#[derive",
    "thiserror",
    "anyhow",
    "?;",
    "cargo ",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdiomMismatchConfig {
    pub min_foreign_hits: usize,
    pub max_rust_hits_for_flag: usize,
}

impl Default for IdiomMismatchConfig {
    fn default() -> Self {
        Self {
            min_foreign_hits: 3,
            max_rust_hits_for_flag: 2,
        }
    }
}

pub fn detect_file(path: impl AsRef<Path>) -> Result<Vec<Finding>, PapertowelError> {
    let path = path.as_ref();
    let content =
        fs::read_to_string(path).map_err(|error| PapertowelError::io_with_path(path, error))?;
    detect_in_text(path, &content, IdiomMismatchConfig::default())
}

#[expect(
    clippy::cast_precision_loss,
    reason = "confidence score: bounded usize counts"
)]
pub fn detect_in_text(
    file_path: impl Into<PathBuf>,
    content: &str,
    config: IdiomMismatchConfig,
) -> Result<Vec<Finding>, PapertowelError> {
    let file_path = file_path.into();

    let is_rust_target = file_path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"));

    if !is_rust_target {
        return Ok(Vec::new());
    }

    let lowered = content.to_ascii_lowercase();
    let foreign_hits = FOREIGN_IDIOM_MARKERS
        .iter()
        .filter(|marker| lowered.contains(**marker))
        .count();
    let rust_hits = RUST_IDIOM_MARKERS
        .iter()
        .filter(|marker| lowered.contains(**marker))
        .count();

    if foreign_hits < config.min_foreign_hits || rust_hits > config.max_rust_hits_for_flag {
        return Ok(Vec::new());
    }

    let severity = if foreign_hits >= 6 {
        Severity::High
    } else {
        Severity::Medium
    };
    let confidence = (foreign_hits as f32 / 8.0)
        .mul_add(
            0.7,
            ((config.max_rust_hits_for_flag.saturating_sub(rust_hits)) as f32
                / config.max_rust_hits_for_flag.max(1) as f32)
                * 0.3,
        )
        .min(1.0);

    let end_line = content.lines().count().max(1);
    let mut finding = Finding::new(
        "idiom.cross_language.mismatch",
        FindingCategory::IdiomMismatch,
        severity,
        confidence,
        file_path,
        format!(
            "Detected cross-language idiom mismatch (foreign markers: {foreign_hits}, Rust markers: {rust_hits})."
        ),
    )?;
    finding.line_range = Some(LineRange::new(1, end_line)?);
    finding.suggestion = Some(
		"Refactor toward idiomatic Rust patterns (`Result`, `match`, ownership-aware APIs) and remove cross-language tutorial residue."
			.to_owned(),
	);

    Ok(vec![finding])
}

#[cfg(test)]
mod tests {
    use crate::scrubber::idiom_mismatch::{DETECTOR_NAME, IdiomMismatchConfig, detect_in_text};

    #[test]
    fn detector_name_is_stable() {
        assert_eq!(DETECTOR_NAME, "idiom_mismatch");
    }

    #[test]
    fn idiom_detector_ignores_non_rust_files() -> Result<(), Box<dyn std::error::Error>> {
        let findings = detect_in_text(
            "script.py",
            "def run():\n    print('hello')\n",
            IdiomMismatchConfig::default(),
        )?;
        assert!(findings.is_empty());
        Ok(())
    }

    #[test]
    fn idiom_detector_ignores_idiomatic_rust() -> Result<(), Box<dyn std::error::Error>> {
        let content = "fn run() -> Result<(), anyhow::Error> { if let Some(x) = Some(1) { println!(\"{x}\"); } Ok(()) }";
        let findings = detect_in_text("src/lib.rs", content, IdiomMismatchConfig::default())?;
        assert!(findings.is_empty());
        Ok(())
    }

    #[test]
    fn idiom_detector_flags_cross_language_rust_file() -> Result<(), Box<dyn std::error::Error>> {
        let content = "\
package main\n\
public static void main(String[] args) {}\n\
console.log('hello')\n\
fmt.println(\"hello\")\n\
pip install foo\n\
";
        let findings = detect_in_text("src/lib.rs", content, IdiomMismatchConfig::default())?;
        assert_eq!(findings.len(), 1);
        Ok(())
    }
}
