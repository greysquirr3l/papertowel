/// Safety guard configuration for scrub write operations.
///
/// All ratio fields are stored as integer percentages (0–100) so that
/// the type can derive `Eq` and be embedded in config structs cleanly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SafetyConfig {
    /// Minimum ratio of output bytes to input bytes, as a percentage.
    ///
    /// If the scrubbed content is smaller than this fraction of the original,
    /// the write is blocked.  Default: 50 (50 %).
    pub min_size_percent: u8,

    /// Maximum fraction of original lines that may be dropped, as a
    /// percentage.
    ///
    /// If more lines are removed than this fraction allows, the write is
    /// blocked.  Default: 60 (60 %).
    pub max_line_drop_percent: u8,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            min_size_percent: 50,
            max_line_drop_percent: 60,
        }
    }
}

/// Outcome of a safety-guard check.
#[derive(Debug)]
pub enum SafetyOutcome {
    /// Transformation is within thresholds; proceed with the write.
    Allowed,
    /// Transformation violates a threshold.  The inner [`String`] is a
    /// human-readable reason suitable for a warning or report entry.
    Blocked(String),
}

/// Check whether transforming `original` into `transformed` is safe according
/// to `config`.
///
/// Returns [`SafetyOutcome::Allowed`] when the content is empty (nothing to
/// protect) or when all thresholds are satisfied.
pub fn check_safety(original: &str, transformed: &str, config: &SafetyConfig) -> SafetyOutcome {
    let orig_bytes = original.len();

    // Empty original — nothing to protect; always allow.
    if orig_bytes == 0 {
        return SafetyOutcome::Allowed;
    }

    // Byte-size ratio check.
    let new_bytes = transformed.len();
    let size_ratio = (new_bytes as f64) / (orig_bytes as f64);
    let min_ratio = f64::from(config.min_size_percent) / 100.0;
    if size_ratio < min_ratio {
        return SafetyOutcome::Blocked(format!(
            "output is {:.0}% of original size (minimum: {}%)",
            size_ratio * 100.0,
            config.min_size_percent,
        ));
    }

    // Line-drop ratio check.
    let orig_lines = original.lines().count();
    if orig_lines > 0 {
        let new_lines = transformed.lines().count();
        let lines_dropped = orig_lines.saturating_sub(new_lines);
        let drop_ratio = (lines_dropped as f64) / (orig_lines as f64);
        let max_ratio = f64::from(config.max_line_drop_percent) / 100.0;
        if drop_ratio > max_ratio {
            return SafetyOutcome::Blocked(format!(
                "dropped {lines_dropped}/{orig_lines} lines ({:.0}%); maximum allowed: {}%",
                drop_ratio * 100.0,
                config.max_line_drop_percent,
            ));
        }
    }

    SafetyOutcome::Allowed
}

#[cfg(test)]
mod tests {
    use super::{SafetyConfig, SafetyOutcome, check_safety};

    fn cfg(min_size: u8, max_drop: u8) -> SafetyConfig {
        SafetyConfig {
            min_size_percent: min_size,
            max_line_drop_percent: max_drop,
        }
    }

    fn is_blocked(o: SafetyOutcome) -> bool {
        matches!(o, SafetyOutcome::Blocked(_))
    }

    #[test]
    fn empty_original_always_allows() {
        let outcome = check_safety("", "whatever", &SafetyConfig::default());
        assert!(matches!(outcome, SafetyOutcome::Allowed));
    }

    #[test]
    fn safe_transform_passes() {
        let original = "fn main() {\n    println!(\"hello\");\n}\n";
        // Remove one comment — well within defaults.
        let transformed = "fn main() {\n    println!(\"hello\");\n}\n";
        assert!(matches!(
            check_safety(original, transformed, &SafetyConfig::default()),
            SafetyOutcome::Allowed
        ));
    }

    #[test]
    fn over_aggressive_size_reduction_is_blocked() {
        let original = "a".repeat(1000);
        let transformed = "a".repeat(100); // 10% of original
        let outcome = check_safety(&original, &transformed, &cfg(50, 60));
        assert!(is_blocked(outcome));
    }

    #[test]
    fn size_exactly_at_threshold_passes() {
        let original = "a".repeat(1000);
        let transformed = "a".repeat(500); // exactly 50%
        let outcome = check_safety(&original, &transformed, &cfg(50, 60));
        assert!(matches!(outcome, SafetyOutcome::Allowed));
    }

    #[test]
    fn over_aggressive_line_drop_is_blocked() {
        let original = (0..100).map(|i| format!("line {i}\n")).collect::<String>();
        // Drop 80 lines — above 60% threshold.
        let transformed = (0..20).map(|i| format!("line {i}\n")).collect::<String>();
        let outcome = check_safety(&original, &transformed, &cfg(10, 60));
        assert!(is_blocked(outcome));
    }

    #[test]
    fn line_drop_exactly_at_threshold_passes() {
        let original = (0..100).map(|i| format!("line {i}\n")).collect::<String>();
        // Drop exactly 60 lines — exactly at threshold.
        let transformed = (0..40).map(|i| format!("line {i}\n")).collect::<String>();
        let outcome = check_safety(&original, &transformed, &cfg(10, 60));
        assert!(matches!(outcome, SafetyOutcome::Allowed));
    }

    #[test]
    fn default_config_has_expected_values() {
        let cfg = SafetyConfig::default();
        assert_eq!(cfg.min_size_percent, 50);
        assert_eq!(cfg.max_line_drop_percent, 60);
    }
}
