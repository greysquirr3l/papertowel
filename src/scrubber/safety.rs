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

    // Byte-size ratio check using integer percent math to avoid float casts.
    let new_bytes = transformed.len();
    let size_percent = ratio_percent(new_bytes, orig_bytes);
    if size_percent < config.min_size_percent {
        return SafetyOutcome::Blocked(format!(
            "output is {}% of original size (minimum: {}%)",
            size_percent, config.min_size_percent,
        ));
    }

    // Line-drop ratio check.
    let orig_lines = original.lines().count();
    if orig_lines > 0 {
        let new_lines = transformed.lines().count();
        let lines_dropped = orig_lines.saturating_sub(new_lines);
        let drop_percent = ratio_percent(lines_dropped, orig_lines);
        if drop_percent > config.max_line_drop_percent {
            return SafetyOutcome::Blocked(format!(
                "dropped {lines_dropped}/{orig_lines} lines ({}%); maximum allowed: {}%",
                drop_percent, config.max_line_drop_percent,
            ));
        }
    }

    SafetyOutcome::Allowed
}

fn ratio_percent(part: usize, whole: usize) -> u8 {
    if whole == 0 {
        return 0;
    }

    // Rounded to nearest integer percent.
    // Rounded integer percentage using wider math to avoid overflow.
    let part_u128 = part as u128;
    let whole_u128 = whole as u128;
    let pct = ((part_u128 * 100) + (whole_u128 / 2)) / whole_u128;
    let clamped = pct.min(100);
    u8::try_from(clamped).unwrap_or(100)
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

    fn is_blocked(outcome: &SafetyOutcome) -> bool {
        matches!(outcome, SafetyOutcome::Blocked(_))
    }

    fn build_lines(start: usize, end: usize) -> String {
        let mut out = String::new();
        for i in start..end {
            out.push_str("line ");
            out.push_str(&i.to_string());
            out.push('\n');
        }
        out
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
        assert!(is_blocked(&outcome));
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
        let original = build_lines(0, 100);
        // Drop 80 lines — above 60% threshold.
        let transformed = build_lines(0, 20);
        let outcome = check_safety(&original, &transformed, &cfg(10, 60));
        assert!(is_blocked(&outcome));
    }

    #[test]
    fn line_drop_exactly_at_threshold_passes() {
        let original = build_lines(0, 100);
        // Drop exactly 60 lines — exactly at threshold.
        let transformed = build_lines(0, 40);
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
