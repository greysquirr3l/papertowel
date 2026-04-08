use crate::detection::finding::Severity;

#[derive(Debug, Clone, Copy)]
pub struct DetectionThresholds {
    pub low: f32,
    pub medium: f32,
    pub high: f32,
}

impl Default for DetectionThresholds {
    fn default() -> Self {
        Self {
            low: 2.5,
            medium: 5.0,
            high: 8.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ScoreBreakdown {
    pub file_score: f32,
    pub repo_score: f32,
    pub history_score: f32,
}

impl ScoreBreakdown {
    #[must_use]
    pub fn total(&self) -> f32 {
        self.file_score + self.repo_score + self.history_score
    }

    #[must_use]
    pub fn classify(&self, thresholds: DetectionThresholds) -> Severity {
        let total = self.total();
        if total >= thresholds.high {
            Severity::High
        } else if total >= thresholds.medium {
            Severity::Medium
        } else {
            Severity::Low
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DetectionThresholds, ScoreBreakdown};
    use crate::detection::finding::Severity;

    #[test]
    fn score_breakdown_total_sums_components() {
        let score = ScoreBreakdown {
            file_score: 1.5,
            repo_score: 2.0,
            history_score: 0.5,
        };

        assert_eq!(score.total(), 4.0);
    }

    #[test]
    fn classify_maps_to_expected_severity() {
        let thresholds = DetectionThresholds::default();
        let low = ScoreBreakdown {
            file_score: 1.0,
            repo_score: 0.0,
            history_score: 0.0,
        };
        let medium = ScoreBreakdown {
            file_score: 3.0,
            repo_score: 2.0,
            history_score: 0.0,
        };
        let high = ScoreBreakdown {
            file_score: 4.0,
            repo_score: 3.0,
            history_score: 2.0,
        };

        assert_eq!(low.classify(thresholds), Severity::Low);
        assert_eq!(medium.classify(thresholds), Severity::Medium);
        assert_eq!(high.classify(thresholds), Severity::High);
    }
}