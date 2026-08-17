//! Confidence-tier classification.
//!
//! Maps a `Finding`'s raw `confidence_score` (in `0.0..1.0`) to a coarse
//! tier that downstream consumers (grade weighting, reporting, future
//! filtering) can act on without re-implementing the thresholds.
//!
//! Thresholds mirror `watermarks-remover`'s
//! `classify_finding_confidence()` upstream:
//!
//! | Score | Tier |
//! |---|---|
//! | ≥ 0.95 | Clean |
//! | 0.80..=0.95 | Low |
//! | 0.65..=0.80 | Medium |
//! | < 0.65 or `Severity::High` | High |
//!
//! Each tier carries a `grade_multiplier` used by `detection::grading`
//! to modulate severity-weighted finding impact on the project score.

use serde::{Deserialize, Serialize};

use crate::detection::finding::Severity;
use crate::domain::errors::PapertowelError;

/// Coarse confidence tier derived from a `Finding`'s `confidence_score`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfidenceTier {
    /// ≥ 0.95 confidence OR no findings. Suppresses grade impact.
    Clean,
    /// 0.80..=0.95 confidence.
    Low,
    /// 0.65..=0.80 confidence.
    Medium,
    /// < 0.65 confidence, or severity == High.
    High,
}

impl ConfidenceTier {
    /// Classify a finding by raw confidence score and severity.
    ///
    /// `Severity::High` always escalates to `High` regardless of
    /// confidence - a high-severity finding can never be a "clean" or
    /// "low" signal.
    #[must_use]
    pub const fn classify(confidence_score: f32, severity: Severity) -> Self {
        if matches!(severity, Severity::High) {
            return Self::High;
        }
        if confidence_score >= 0.95 {
            Self::Clean
        } else if confidence_score >= 0.80 {
            Self::Low
        } else if confidence_score >= 0.65 {
            Self::Medium
        } else {
            Self::High
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    /// Grade-impact multiplier for severity-weighted scoring.
    ///
    /// `Clean` findings contribute 0.0× their severity weight (they're
    /// suppressed). `High` findings contribute 1.5× (over-weighted to
    /// reflect the high-stakes signal).
    #[must_use]
    pub const fn grade_multiplier(self) -> f32 {
        match self {
            Self::Clean => 0.0,
            Self::Low => 0.5,
            Self::Medium => 1.0,
            Self::High => 1.5,
        }
    }
}

impl std::fmt::Display for ConfidenceTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ConfidenceTier {
    type Err = PapertowelError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "clean" => Ok(Self::Clean),
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            other => Err(PapertowelError::Validation(format!(
                "unknown confidence tier '{other}'"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_mapping() {
        assert_eq!(ConfidenceTier::classify(1.0, Severity::Low), ConfidenceTier::Clean);
        assert_eq!(ConfidenceTier::classify(0.95, Severity::Low), ConfidenceTier::Clean);
        assert_eq!(ConfidenceTier::classify(0.949, Severity::Low), ConfidenceTier::Low);
        assert_eq!(ConfidenceTier::classify(0.80, Severity::Low), ConfidenceTier::Low);
        assert_eq!(ConfidenceTier::classify(0.799, Severity::Low), ConfidenceTier::Medium);
        assert_eq!(ConfidenceTier::classify(0.65, Severity::Low), ConfidenceTier::Medium);
        assert_eq!(ConfidenceTier::classify(0.649, Severity::Low), ConfidenceTier::High);
    }

    #[test]
    fn high_severity_always_escalates_to_high() {
        assert_eq!(ConfidenceTier::classify(1.0, Severity::High), ConfidenceTier::High);
        assert_eq!(ConfidenceTier::classify(0.95, Severity::High), ConfidenceTier::High);
        assert_eq!(ConfidenceTier::classify(0.80, Severity::High), ConfidenceTier::High);
    }

    #[test]
    fn grade_multipliers_match_doc() {
        let c = ConfidenceTier::Clean.grade_multiplier();
        assert!(c.abs() < f32::EPSILON, "Clean: {c}");
        assert!((ConfidenceTier::Low.grade_multiplier() - 0.5).abs() < f32::EPSILON);
        assert!((ConfidenceTier::Medium.grade_multiplier() - 1.0).abs() < f32::EPSILON);
        assert!((ConfidenceTier::High.grade_multiplier() - 1.5).abs() < f32::EPSILON);
    }

    #[test]
    fn from_str_roundtrip() -> Result<(), crate::domain::errors::PapertowelError> {
        for tier in [
            ConfidenceTier::Clean,
            ConfidenceTier::Low,
            ConfidenceTier::Medium,
            ConfidenceTier::High,
        ] {
            assert_eq!(tier.as_str().parse::<ConfidenceTier>()?, tier);
        }
        Ok(())
    }
}
