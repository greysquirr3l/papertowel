use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use walkdir::WalkDir;

use crate::detection::finding::{Finding, FindingCategory, LineRange, Severity};
use crate::domain::errors::PapertowelError;

pub const DETECTOR_NAME: &str = "promotion";

const PROMOTIONAL_MARKERS: [&str; 12] = [
    "revolutionary",
    "game-changing",
    "next-generation",
    "enterprise-ready",
    "best-in-class",
    "production-ready",
    "launching",
    "viral",
    "one-click",
    "instant",
    "showcase",
    "demo",
];

const CODE_EXTENSIONS: [&str; 8] = ["rs", "go", "py", "ts", "tsx", "js", "cs", "zig"];
const IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "svg"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromotionDetectionConfig {
    pub min_promotional_hits: usize,
    pub min_image_count: usize,
    pub max_code_files_for_flag: usize,
}

impl Default for PromotionDetectionConfig {
    fn default() -> Self {
        Self {
            min_promotional_hits: 4,
            min_image_count: 2,
            max_code_files_for_flag: 4,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct RepoPromoMetrics {
    code_files: usize,
    image_files: usize,
    promotional_hits: usize,
}

pub fn detect_repo(repo_root: impl AsRef<Path>) -> Result<Vec<Finding>, PapertowelError> {
    detect_repo_with_config(repo_root, PromotionDetectionConfig::default())
}

#[expect(clippy::cast_precision_loss, reason = "confidence score: bounded usize counts")]
pub fn detect_repo_with_config(
    repo_root: impl AsRef<Path>,
    config: PromotionDetectionConfig,
) -> Result<Vec<Finding>, PapertowelError> {
    let repo_root = repo_root.as_ref();
    let metrics = scan_promotional_shape(repo_root)?;

    if metrics.promotional_hits < config.min_promotional_hits
        || metrics.image_files < config.min_image_count
        || metrics.code_files > config.max_code_files_for_flag
    {
        return Ok(Vec::new());
    }

    let severity = if metrics.promotional_hits >= 7 && metrics.code_files <= 2 {
        Severity::High
    } else {
        Severity::Medium
    };

    let confidence = (metrics.promotional_hits as f32 / 10.0)
        .mul_add(
            0.5,
            (metrics.image_files as f32 / 6.0).mul_add(
                0.3,
                ((config
                    .max_code_files_for_flag
                    .saturating_sub(metrics.code_files)) as f32
                    / config.max_code_files_for_flag.max(1) as f32)
                    * 0.2,
            ),
        )
        .min(1.0);

    let mut finding = Finding::new(
        "promotion.showcase_stack",
        FindingCategory::Promotion,
        severity,
        confidence,
        repo_root.join("README.md"),
        format!(
            "Detected promotion-heavy repository shape (promo hits: {}, images: {}, code files: {}).",
            metrics.promotional_hits, metrics.image_files, metrics.code_files
        ),
    )?;
    finding.line_range = Some(LineRange::new(1, 1)?);
    finding.suggestion = Some(
		"Balance promotional copy with concrete engineering artifacts: deeper implementation docs, tests, and meaningful code evolution."
			.to_owned(),
	);

    Ok(vec![finding])
}

fn scan_promotional_shape(repo_root: &Path) -> Result<RepoPromoMetrics, PapertowelError> {
    let mut metrics = RepoPromoMetrics::default();

    let readme = repo_root.join("README.md");
    if readme.is_file() {
        let content = fs::read_to_string(&readme)
            .map_err(|error| PapertowelError::io_with_path(&readme, error))?;
        let lowered = content.to_ascii_lowercase();
        metrics.promotional_hits = PROMOTIONAL_MARKERS
            .iter()
            .filter(|marker| lowered.contains(**marker))
            .count();
    }

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

        if !path.is_file() {
            continue;
        }

        let Some(ext) = path.extension().and_then(OsStr::to_str) else {
            continue;
        };

        if CODE_EXTENSIONS.contains(&ext) {
            metrics.code_files += 1;
        } else if IMAGE_EXTENSIONS.contains(&ext) {
            metrics.image_files += 1;
        }
    }

    Ok(metrics)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use crate::scrubber::promotion::{
        DETECTOR_NAME, PromotionDetectionConfig, detect_repo_with_config,
    };

    #[test]
    fn detector_name_is_stable() {
        assert_eq!(DETECTOR_NAME, "promotion");
    }

    #[test]
    fn promotion_detector_ignores_balanced_repo() -> Result<(), Box<dyn std::error::Error>> {
        let temp = TempDir::new()?;

        let src = temp.path().join("src");
        fs::create_dir_all(&src)?;
        fs::write(src.join("lib.rs"), "pub fn run() {}\n")?;
        fs::write(src.join("engine.rs"), "pub fn work() {}\n")?;
        fs::write(
            temp.path().join("README.md"),
            "Technical architecture and usage details.\n",
        )?;

        let findings = detect_repo_with_config(temp.path(), PromotionDetectionConfig::default())?;
        assert!(findings.is_empty());
        Ok(())
    }

    #[test]
    fn promotion_detector_flags_showcase_stack() -> Result<(), Box<dyn std::error::Error>> {
        let temp = TempDir::new()?;

        let assets = temp.path().join("assets");
        let src = temp.path().join("src");
        fs::create_dir_all(&assets)?;
        fs::create_dir_all(&src)?;
        fs::write(src.join("main.rs"), "fn main() {}\n")?;

        fs::write(assets.join("hero.png"), "binary")?;
        fs::write(assets.join("demo.gif"), "binary")?;
        fs::write(
			temp.path().join("README.md"),
			"Revolutionary next-generation launch demo. Best-in-class and production-ready one-click showcase."
		)?;

        let findings = detect_repo_with_config(temp.path(), PromotionDetectionConfig::default())?;
        assert_eq!(findings.len(), 1);
        Ok(())
    }
}
