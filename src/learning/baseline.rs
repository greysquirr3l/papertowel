use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::domain::errors::PapertowelError;

/// Commit-cadence statistics extracted from git history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CommitStats {
    /// Total commits analysed.
    pub commits_analysed: usize,
    /// Average hour-of-day when commits land (0–23, local author time).
    pub avg_commit_hour: f32,
    /// Which weekdays are active (0 = Mon … 6 = Sun), represented as a
    /// fraction of commits that fell on each day.
    pub weekday_distribution: [f32; 7],
    /// Average commit-message length in characters.
    pub avg_message_length: f32,
    /// Fraction of messages that follow Conventional Commits convention
    /// (`type(scope)?:` prefix).
    pub conventional_commit_rate: f32,
    /// Fraction of messages that look like WIP / fixup messages.
    pub wip_message_rate: f32,
}

/// A learned style baseline for a repository owner.
///
/// Stores aggregate statistics extracted from existing source code so that
/// papertowel can calibrate detector thresholds to a user's natural style
/// instead of using a one-size-fits-all heuristic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StyleBaseline {
    /// Fraction of non-empty lines that are comments (0.0–1.0).
    pub avg_comment_density: f32,
    /// Fraction of comment lines that are doc-comments (0.0–1.0).
    pub avg_doc_ratio: f32,
    /// Average number of slop-vocabulary hits per 100 lines.
    pub slop_rate_per_hundred: f32,
    /// Number of source files that were analysed.
    pub files_analyzed: usize,
    /// Number of source lines (non-empty) that were analysed.
    pub lines_analyzed: usize,
    /// Unix timestamp (seconds) when the baseline was recorded.
    pub created_at: u64,
    /// Commit-cadence statistics derived from git history (absent when the
    /// repo has no commits or is not a git repository).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_stats: Option<CommitStats>,
}

impl StyleBaseline {
    /// Derive a calibrated `high_density_threshold` from this baseline.
    ///
    /// The threshold is set to the user's observed comment density plus a
    /// 50 % margin, clamped to [0.20, 0.80].  This prevents the detector from
    /// firing on heavily-commented codebases that happen to be human-written.
    #[must_use]
    pub fn comment_density_threshold(&self) -> f32 {
        (self.avg_comment_density * 1.5).clamp(0.20, 0.80)
    }

    /// Return the path where the baseline is persisted relative to a repo root.
    #[must_use]
    pub fn relative_path() -> &'static Path {
        Path::new(".papertowel/baseline.toml")
    }

    /// Load a baseline from `<root>/.papertowel/baseline.toml`.
    ///
    /// Returns `Ok(None)` when the file does not exist (no baseline recorded
    /// yet).
    pub fn load(root: &Path) -> Result<Option<Self>, PapertowelError> {
        let path = root.join(Self::relative_path());
        if !path.exists() {
            return Ok(None);
        }
        let text =
            std::fs::read_to_string(&path).map_err(|e| PapertowelError::io_with_path(&path, e))?;
        let baseline: Self =
            toml::from_str(&text).map_err(|e| PapertowelError::Config(e.to_string()))?;
        Ok(Some(baseline))
    }

    /// Persist the baseline to `<root>/.papertowel/baseline.toml`.
    pub fn save(&self, root: &Path) -> Result<PathBuf, PapertowelError> {
        let dir = root.join(".papertowel");
        std::fs::create_dir_all(&dir).map_err(|e| PapertowelError::io_with_path(&dir, e))?;
        let path = dir.join("baseline.toml");
        let text =
            toml::to_string_pretty(self).map_err(|e| PapertowelError::Config(e.to_string()))?;
        std::fs::write(&path, text).map_err(|e| PapertowelError::io_with_path(&path, e))?;
        Ok(path)
    }
}

/// Return the current Unix timestamp in seconds, falling back to 0 on error.
pub(super) fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::StyleBaseline;

    #[test]
    fn comment_density_threshold_lower_bound() {
        let b = StyleBaseline {
            avg_comment_density: 0.0,
            avg_doc_ratio: 0.0,
            slop_rate_per_hundred: 0.0,
            files_analyzed: 0,
            lines_analyzed: 0,
            created_at: 0,
            commit_stats: None,
        };
        assert!(b.comment_density_threshold() >= 0.20);
    }

    #[test]
    fn comment_density_threshold_upper_bound() {
        let b = StyleBaseline {
            avg_comment_density: 1.0,
            avg_doc_ratio: 0.0,
            slop_rate_per_hundred: 0.0,
            files_analyzed: 0,
            lines_analyzed: 0,
            created_at: 0,
            commit_stats: None,
        };
        assert!(b.comment_density_threshold() <= 0.80);
    }

    #[test]
    fn comment_density_threshold_scales_with_density() {
        let low = StyleBaseline {
            avg_comment_density: 0.10,
            avg_doc_ratio: 0.0,
            slop_rate_per_hundred: 0.0,
            files_analyzed: 1,
            lines_analyzed: 100,
            created_at: 0,
            commit_stats: None,
        };
        let high = StyleBaseline {
            avg_comment_density: 0.40,
            avg_doc_ratio: 0.0,
            slop_rate_per_hundred: 0.0,
            files_analyzed: 1,
            lines_analyzed: 100,
            created_at: 0,
            commit_stats: None,
        };
        assert!(
            high.comment_density_threshold() > low.comment_density_threshold(),
            "denser baseline → higher threshold"
        );
    }

    #[test]
    fn save_and_load_roundtrip() {
        use tempfile::TempDir;
        let tmp = TempDir::new().expect("tempdir");
        let baseline = StyleBaseline {
            avg_comment_density: 0.12,
            avg_doc_ratio: 0.35,
            slop_rate_per_hundred: 2.5,
            files_analyzed: 42,
            lines_analyzed: 1234,
            created_at: 9999,
            commit_stats: Some(super::CommitStats {
                commits_analysed: 30,
                avg_commit_hour: 22.5,
                weekday_distribution: [0.1, 0.2, 0.15, 0.15, 0.1, 0.2, 0.1],
                avg_message_length: 48.0,
                conventional_commit_rate: 0.9,
                wip_message_rate: 0.05,
            }),
        };
        let path = baseline.save(tmp.path()).expect("save");
        assert!(path.exists());
        let loaded = StyleBaseline::load(tmp.path())
            .expect("load")
            .expect("some");
        assert_eq!(loaded.files_analyzed, 42);
        assert_eq!(loaded.lines_analyzed, 1234);
        assert!(loaded.commit_stats.is_some());
        assert_eq!(
            loaded
                .commit_stats
                .as_ref()
                .expect("stats")
                .commits_analysed,
            30
        );
    }

    #[test]
    fn load_returns_none_when_file_absent() {
        use tempfile::TempDir;
        let tmp = TempDir::new().expect("tempdir");
        let result = StyleBaseline::load(tmp.path()).expect("no error");
        assert!(result.is_none(), "absent baseline should return None");
    }
}
