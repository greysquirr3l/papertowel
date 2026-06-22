/// RFC 5424 syslog severity levels accepted by `logging/setLevel`.
pub const LEVELS: &[&str] = &[
    "debug", "info", "notice", "warning", "error", "critical", "alert", "emergency",
];

/// Validate and return the level string if it is in the accepted set.
///
/// `None` is returned for unknown levels so the caller can map to
/// `-32602 Invalid Params`.
#[must_use]
pub fn set_log_level(level: &str) -> Option<&'static str> {
    LEVELS.iter().find(|l| **l == level).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_syslog_levels() {
        for level in LEVELS {
            assert_eq!(set_log_level(level), Some(*level));
        }
    }

    #[test]
    fn rejects_unknown_levels() {
        assert_eq!(set_log_level("verbose"), None);
        assert_eq!(set_log_level(""), None);
    }
}