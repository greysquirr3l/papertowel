//! Architecture quality detector.
//!
//! Detects signs of AI-generated code that lacks coherent architectural patterns.
//! Well-structured code typically follows patterns like:
//! - Hexagonal/ports-and-adapters
//! - Domain-Driven Design (DDD) lite
//! - Clean Architecture
//! - CQRS (Command Query Responsibility Segregation)
//!
//! AI-generated code tends to:
//! - Dump everything in flat files
//! - Mix concerns (business logic with I/O)
//! - Create anemic domain models (data-only structs)
//! - Skip abstractions (no traits as ports)
//! - Use god files with too many responsibilities

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use walkdir::WalkDir;

use crate::detection::finding::{Finding, FindingCategory, Severity};
use crate::domain::errors::PapertowelError;

pub const DETECTOR_NAME: &str = "architecture";

/// Configuration for architecture detection thresholds.
#[derive(Debug, Clone, Copy)]
pub struct ArchitectureConfig {
    /// Minimum source files to trigger analysis (small projects get a pass).
    pub min_source_files: usize,
    /// Lines threshold for "god file" detection.
    pub god_file_lines: usize,
    /// Minimum trait count expected for abstraction.
    pub min_trait_ratio: f32,
    /// Maximum fraction of structs that are anemic (no methods).
    pub max_anemic_ratio: f32,
    /// Minimum directory depth for non-flat structure.
    pub min_directory_depth: usize,
}

impl Default for ArchitectureConfig {
    fn default() -> Self {
        Self {
            min_source_files: 8,
            god_file_lines: 800, // 800 lines is reasonable for Rust with inline tests
            min_trait_ratio: 0.02, // 2% is low bar — CLI tools may have none
            max_anemic_ratio: 0.80, // More than 80% anemic structs is suspicious
            min_directory_depth: 2, // Expect at least src/domain/ style nesting
        }
    }
}

/// Metrics collected from repo architecture analysis.
#[derive(Debug, Clone, Default)]
pub struct ArchitectureMetrics {
    pub total_source_files: usize,
    pub total_lines: usize,
    pub max_file_lines: usize,
    pub max_file_path: Option<PathBuf>,
    pub directory_depth: usize,
    pub has_layer_structure: bool,
    pub layer_dirs_found: Vec<String>,
    pub trait_count: usize,
    pub struct_count: usize,
    pub anemic_struct_count: usize,
    pub impl_block_count: usize,
    pub god_files: Vec<PathBuf>,
    pub flat_structure: bool,
}

impl ArchitectureMetrics {
    #[expect(
        clippy::cast_precision_loss,
        reason = "bounded counts from small repos; f32 precision is sufficient for ratio"
    )]
    fn trait_ratio(&self) -> f32 {
        let total_types = self.trait_count + self.struct_count;
        if total_types == 0 {
            return 0.0;
        }
        self.trait_count as f32 / total_types as f32
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "bounded counts from small repos; f32 precision is sufficient for ratio"
    )]
    fn anemic_ratio(&self) -> f32 {
        if self.struct_count == 0 {
            return 0.0;
        }
        self.anemic_struct_count as f32 / self.struct_count as f32
    }
}

// ─── Layer Detection ─────────────────────────────────────────────────────────

/// Common directory names indicating architectural layers.
const LAYER_INDICATORS: &[&str] = &[
    // DDD / Clean Architecture
    "domain",
    "application",
    "infrastructure",
    "presentation",
    // Hexagonal
    "ports",
    "adapters",
    "core",
    // CQRS
    "commands",
    "queries",
    "handlers",
    // Common patterns
    "services",
    "repositories",
    "entities",
    "aggregates",
    "value_objects",
    "events",
    "use_cases",
    "interfaces",
    // Rust/CLI conventions
    "cli",
    "api",
    "lib",
    "config",
    "models",
    "utils",
];

/// Minimum layers to consider "structured".
const MIN_LAYER_DIRS: usize = 2;

// ─── Public API ──────────────────────────────────────────────────────────────

/// Analyze repository architecture and return findings.
pub fn detect_repo(root: impl AsRef<Path>) -> Result<Vec<Finding>, PapertowelError> {
    detect_repo_with_config(root, ArchitectureConfig::default())
}

/// Analyze with custom configuration.
#[expect(
    clippy::too_many_lines,
    reason = "Cohesive detection logic; splitting would fragment readability"
)]
pub fn detect_repo_with_config(
    root: impl AsRef<Path>,
    config: ArchitectureConfig,
) -> Result<Vec<Finding>, PapertowelError> {
    let root = root.as_ref();
    let metrics = analyze_repo(root, &config)?;

    if metrics.total_source_files < config.min_source_files {
        return Ok(Vec::new()); // Too small to judge
    }

    let mut findings = Vec::new();
    let repo_path = PathBuf::from(".");

    // Check for flat structure (no meaningful subdirectories)
    if metrics.flat_structure {
        let mut finding = Finding::new(
            "ARCH001",
            FindingCategory::Architecture,
            Severity::Medium,
            0.75,
            repo_path.clone(),
            "Flat module structure detected. AI-generated code often lacks \
             organized subdirectories (domain/, infrastructure/, etc.).",
        )?;
        finding.suggestion = Some(
            "Consider organizing code into layers: domain/, application/, \
             infrastructure/, or ports/, adapters/, core/."
                .to_owned(),
        );
        findings.push(finding);
    }

    // Check for missing layer structure in larger projects
    if !metrics.has_layer_structure && metrics.total_source_files >= config.min_source_files * 2 {
        let mut finding = Finding::new(
            "ARCH002",
            FindingCategory::Architecture,
            Severity::Medium,
            0.70,
            repo_path.clone(),
            format!(
                "No recognizable architectural layers found. Expected directories \
                 like domain/, ports/, application/, etc. Found {} source files \
                 with no clear separation of concerns.",
                metrics.total_source_files
            ),
        )?;
        finding.suggestion = Some(
            "Adopt a layered architecture pattern (Hexagonal, Clean Architecture, DDD)."
                .to_owned(),
        );
        findings.push(finding);
    }

    // Check for god files
    for god_file in &metrics.god_files {
        let mut finding = Finding::new(
            "ARCH003",
            FindingCategory::Architecture,
            Severity::High,
            0.85,
            god_file.clone(),
            format!(
                "God file detected (>{} lines). Large files mixing multiple \
                 responsibilities are a hallmark of AI-generated code.",
                config.god_file_lines
            ),
        )?;
        finding.suggestion =
            Some("Split into smaller, focused modules with single responsibilities.".to_owned());
        findings.push(finding);
    }

    // Check for missing abstractions (no traits)
    if metrics.trait_ratio() < config.min_trait_ratio && metrics.struct_count >= 5 {
        let mut finding = Finding::new(
            "ARCH004",
            FindingCategory::Architecture,
            Severity::Medium,
            0.65,
            repo_path.clone(),
            format!(
                "Low abstraction ratio: {} traits vs {} structs ({:.0}%). \
                 AI-generated code often skips defining traits as ports/interfaces.",
                metrics.trait_count,
                metrics.struct_count,
                metrics.trait_ratio() * 100.0
            ),
        )?;
        finding.suggestion = Some(
            "Define traits for boundaries (repositories, services, ports) \
             to enable dependency inversion."
                .to_owned(),
        );
        findings.push(finding);
    }

    // Check for anemic domain models
    if metrics.anemic_ratio() > config.max_anemic_ratio && metrics.struct_count >= 5 {
        let mut finding = Finding::new(
            "ARCH005",
            FindingCategory::Architecture,
            Severity::Low,
            0.60,
            repo_path,
            format!(
                "High anemic model ratio: {}/{} structs ({:.0}%) have no methods. \
                 AI tends to generate data-only structs without domain behavior.",
                metrics.anemic_struct_count,
                metrics.struct_count,
                metrics.anemic_ratio() * 100.0
            ),
        )?;
        finding.suggestion = Some(
            "Move behavior into domain types. Structs should encapsulate \
             both data and related operations."
                .to_owned(),
        );
        findings.push(finding);
    }

    Ok(findings)
}

// ─── Analysis Implementation ─────────────────────────────────────────────────

#[expect(
    clippy::cast_precision_loss,
    reason = "bounded counts from small repos; f32 precision is sufficient for heuristics"
)]
#[expect(
    clippy::cast_possible_truncation,
    reason = "result is bounded by struct_count which fits in usize"
)]
#[expect(
    clippy::cast_sign_loss,
    reason = "max(0.0) ensures non-negative result"
)]
fn analyze_repo(root: &Path, config: &ArchitectureConfig) -> Result<ArchitectureMetrics, PapertowelError> {
    let mut metrics = ArchitectureMetrics::default();
    let mut dir_depths: HashSet<usize> = HashSet::new();
    let mut found_layers: HashSet<String> = HashSet::new();

    // Regex patterns for Rust (can extend for other languages)
    let trait_re = Regex::new(r"(?m)^[[:space:]]*(pub\s+)?trait\s+\w+").map_err(|e| {
        PapertowelError::Detection(format!("invalid regex: {e}"))
    })?;
    let struct_re = Regex::new(r"(?m)^[[:space:]]*(pub\s+)?struct\s+\w+").map_err(|e| {
        PapertowelError::Detection(format!("invalid regex: {e}"))
    })?;
    let impl_re = Regex::new(r"(?m)^[[:space:]]*impl\s+").map_err(|e| {
        PapertowelError::Detection(format!("invalid regex: {e}"))
    })?;

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        // Only analyze Rust files for now (extend as needed)
        if ext != "rs" {
            continue;
        }

        // Skip test files, build artifacts, and generated content
        let path_str = path.to_string_lossy();
        if path_str.contains("/target/")
            || path_str.contains("/tests/")
            || path_str.contains("/book/")
            || path_str.contains("/docs/")
            || path_str.contains("/vendor/")
            || path_str.contains("/.git/")
            || path_str.contains("/.coraline/")
            || path_str.ends_with("_test.rs")
        {
            continue;
        }

        metrics.total_source_files += 1;

        // Calculate directory depth relative to root
        if let Ok(rel_path) = path.strip_prefix(root) {
            let depth = rel_path.components().count();
            dir_depths.insert(depth);

            // Check for layer directories
            for component in rel_path.components() {
                let name = component.as_os_str().to_string_lossy().to_lowercase();
                if LAYER_INDICATORS.contains(&name.as_str()) {
                    found_layers.insert(name);
                }
            }
        }

        // Read and analyze file content
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };

        let line_count = content.lines().count();
        metrics.total_lines += line_count;

        if line_count > metrics.max_file_lines {
            metrics.max_file_lines = line_count;
            metrics.max_file_path = Some(path.to_path_buf());
        }

        if line_count > config.god_file_lines {
            metrics.god_files.push(path.to_path_buf());
        }

        // Count traits
        metrics.trait_count += trait_re.find_iter(&content).count();

        // Count structs (simpler approach without tracking names)
        metrics.struct_count += struct_re.find_iter(&content).count();

        // Count impl blocks
        metrics.impl_block_count += impl_re.find_iter(&content).count();
    }

    // Estimate anemic structs: if impl count is much lower than struct count,
    // many structs have no methods. This is a heuristic.
    let impl_coverage = if metrics.struct_count > 0 {
        metrics.impl_block_count as f32 / metrics.struct_count as f32
    } else {
        1.0
    };
    // If less than 0.5 impl blocks per struct on average, estimate half are anemic
    metrics.anemic_struct_count = if impl_coverage < 0.5 {
        (metrics.struct_count as f32 * (1.0 - impl_coverage * 2.0).max(0.0)) as usize
    } else {
        0
    };

    // Determine directory depth
    metrics.directory_depth = dir_depths.iter().copied().max().unwrap_or(0);
    metrics.flat_structure = metrics.directory_depth < config.min_directory_depth
        && metrics.total_source_files >= config.min_source_files;

    // Check for layer structure
    metrics.layer_dirs_found = found_layers.into_iter().collect();
    metrics.has_layer_structure = metrics.layer_dirs_found.len() >= MIN_LAYER_DIRS;

    Ok(metrics)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_trait_ratio_empty() {
        let metrics = ArchitectureMetrics::default();
        assert!((metrics.trait_ratio() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn metrics_trait_ratio_calculates() {
        let metrics = ArchitectureMetrics {
            trait_count: 5,
            struct_count: 15,
            ..Default::default()
        };
        assert!((metrics.trait_ratio() - 0.25).abs() < f32::EPSILON);
    }

    #[test]
    fn metrics_anemic_ratio_calculates() {
        let metrics = ArchitectureMetrics {
            struct_count: 10,
            anemic_struct_count: 8,
            ..Default::default()
        };
        assert!((metrics.anemic_ratio() - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn layer_indicators_include_common_patterns() {
        assert!(LAYER_INDICATORS.contains(&"domain"));
        assert!(LAYER_INDICATORS.contains(&"ports"));
        assert!(LAYER_INDICATORS.contains(&"adapters"));
        assert!(LAYER_INDICATORS.contains(&"commands"));
    }
}
