use std::path::PathBuf;
use std::sync::LazyLock;

use git2::Repository;
use regex::Regex;

use crate::detection::finding::{Finding, FindingCategory, Severity};
use crate::domain::errors::PapertowelError;

pub const DETECTOR_NAME: &str = "commit_pattern";

/// Prefixes that indicate conventional-commit–formatted messages.
static CONVENTIONAL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?:feat|fix|chore|refactor|test|docs|style|ci|build|perf|revert)(?:\([^)]+\))?!?:\s",
    )
    .expect("CONVENTIONAL_RE is a valid regex")
});

/// Terms that appear in normal human "recovery" commits.
const RECOVERY_TERMS: &[&str] = &[
    "wip", "oops", "fixup", "fix:", "fix!: ", "squash", "revert", "undo", "wrong", "mistake",
    "amend", "whoops", "typo", "missed", "forgot",
];

#[derive(Debug, Clone, PartialEq)]
pub struct CommitPatternConfig {
    /// Minimum number of commits required before analysis is attempted.
    pub min_commit_count: usize,
    /// CV of inter-commit gaps (seconds) below this value is flagged as
    /// machine-clean cadence.
    pub max_cadence_cv: f64,
    /// Fraction of conventional-format messages above this value is
    /// a corroborating uniformity signal.
    pub min_conventional_fraction: f64,
}

impl Default for CommitPatternConfig {
    fn default() -> Self {
        Self {
            min_commit_count: 6,
            max_cadence_cv: 0.35,
            min_conventional_fraction: 0.87,
        }
    }
}

/// Metrics computed from a series of commit samples.
#[derive(Debug, Clone, PartialEq)]
pub struct CommitPatternMetrics {
    pub commit_count: usize,
    /// Coefficient of variation of inter-commit gaps in seconds.
    pub cadence_cv: f64,
    /// Fraction of commits with a conventional-format message prefix.
    pub conventional_fraction: f64,
    /// Number of commits carrying a human "recovery" marker.
    pub recovery_commit_count: usize,
}

/// Minimal commit data extracted from the repository for testable analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitSample {
    pub timestamp: i64,
    pub message: String,
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Detect AI-typical commit patterns by walking the repository's HEAD history.
pub fn detect_repo(repo_root: impl Into<PathBuf>) -> Result<Vec<Finding>, PapertowelError> {
    detect_repo_with_config(repo_root, CommitPatternConfig::default())
}

pub fn detect_repo_with_config(
    repo_root: impl Into<PathBuf>,
    config: CommitPatternConfig,
) -> Result<Vec<Finding>, PapertowelError> {
    let repo_root = repo_root.into();
    let repo = Repository::open(&repo_root).map_err(PapertowelError::Git)?;
    let commits = collect_commits(&repo)?;

    let metrics = analyze_commits(&commits);

    if metrics.commit_count < config.min_commit_count {
        return Ok(Vec::new());
    }

    let cadence_uniform = metrics.cadence_cv < config.max_cadence_cv;
    let messages_uniform = metrics.conventional_fraction >= config.min_conventional_fraction;
    let no_recovery = metrics.recovery_commit_count == 0 && metrics.commit_count >= 8;

    let signal_count =
        usize::from(cadence_uniform) + usize::from(messages_uniform) + usize::from(no_recovery);

    if signal_count < 2 {
        return Ok(Vec::new());
    }

    let severity = if signal_count == 3 {
        Severity::High
    } else {
        Severity::Medium
    };

    let confidence = (1.0_f64 - metrics.cadence_cv.min(1.0)) * 0.5
        + metrics.conventional_fraction * 0.3
        + if no_recovery { 0.2 } else { 0.0 };

    let evidence = format!(
        "commits: {}, cadence CV: {:.2}, conventional: {:.0}%, recovery commits: {}",
        metrics.commit_count,
        metrics.cadence_cv,
        metrics.conventional_fraction * 100.0,
        metrics.recovery_commit_count,
    );

    let mut finding = Finding::new(
        "commit_pattern.machine_clean",
        FindingCategory::CommitPattern,
        severity,
        (confidence as f32).clamp(0.0, 1.0),
        repo_root,
        evidence,
    )?;
    finding.suggestion = Some(
        "Mix in occasional fix-up commits, varied message styles, and natural timing gaps."
            .to_owned(),
    );

    Ok(vec![finding])
}

// ─── Public analysis functions ───────────────────────────────────────────────

pub fn analyze_commits(commits: &[CommitSample]) -> CommitPatternMetrics {
    let commit_count = commits.len();

    if commit_count < 2 {
        return CommitPatternMetrics {
            commit_count,
            cadence_cv: 0.0,
            conventional_fraction: 0.0,
            recovery_commit_count: 0,
        };
    }

    // Compute inter-commit gaps (sorted ascending by timestamp first).
    let mut sorted: Vec<i64> = commits.iter().map(|c| c.timestamp).collect();
    sorted.sort_unstable();

    let gaps: Vec<f64> = sorted
        .windows(2)
        .filter_map(|w| {
            let diff = w[1].saturating_sub(w[0]);
            if diff > 0 { Some(diff as f64) } else { None }
        })
        .collect();

    let cadence_cv = coefficient_of_variation(&gaps);

    #[expect(
        clippy::cast_precision_loss,
        reason = "bounded commit count: no meaningful precision loss"
    )]
    let conventional_fraction = commits
        .iter()
        .filter(|c| CONVENTIONAL_RE.is_match(&c.message))
        .count() as f64
        / commit_count as f64;

    let recovery_commit_count = commits
        .iter()
        .filter(|c| {
            let lower = c.message.to_lowercase();
            RECOVERY_TERMS.iter().any(|t| lower.contains(t))
        })
        .count();

    CommitPatternMetrics {
        commit_count,
        cadence_cv,
        conventional_fraction,
        recovery_commit_count,
    }
}

pub fn has_conventional_prefix(message: &str) -> bool {
    CONVENTIONAL_RE.is_match(message)
}

// ─── Internal helpers ────────────────────────────────────────────────────────

fn collect_commits(repo: &Repository) -> Result<Vec<CommitSample>, PapertowelError> {
    let head = repo.head().map_err(PapertowelError::Git)?;
    let head_oid = head
        .target()
        .ok_or_else(|| PapertowelError::Config("HEAD is not a direct reference".to_owned()))?;

    let mut walk = repo.revwalk().map_err(PapertowelError::Git)?;
    walk.push(head_oid).map_err(PapertowelError::Git)?;

    let mut samples = Vec::new();
    for oid_result in walk {
        let oid = oid_result.map_err(PapertowelError::Git)?;
        let commit = repo.find_commit(oid).map_err(PapertowelError::Git)?;

        let message = commit.summary().unwrap_or("").to_owned();
        let timestamp = commit.time().seconds();

        samples.push(CommitSample { timestamp, message });
    }

    Ok(samples)
}

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
    use super::{CommitPatternConfig, CommitSample, analyze_commits, has_conventional_prefix};
    use crate::scrubber::commit_pattern::detect_repo_with_config;

    fn uniform_samples(count: usize, gap_secs: i64) -> Vec<CommitSample> {
        (0..count)
            .map(|i| CommitSample {
                timestamp: 1_700_000_000 + i as i64 * gap_secs,
                message: format!("feat(module{i}): implement feature {i}"),
            })
            .collect()
    }

    fn messy_samples() -> Vec<CommitSample> {
        vec![
            CommitSample {
                timestamp: 1_700_000_000,
                message: "feat: initial setup".to_owned(),
            },
            CommitSample {
                timestamp: 1_700_003_600,
                message: "wip: half done".to_owned(),
            },
            CommitSample {
                timestamp: 1_700_007_200,
                message: "fix: oops missed import".to_owned(),
            },
            CommitSample {
                timestamp: 1_700_014_400,
                message: "refactor: clean up".to_owned(),
            },
            CommitSample {
                timestamp: 1_700_025_000,
                message: "revert previous change".to_owned(),
            },
            CommitSample {
                timestamp: 1_700_040_000,
                message: "chore: misc".to_owned(),
            },
            CommitSample {
                timestamp: 1_700_090_000,
                message: "feat: finish module".to_owned(),
            },
            CommitSample {
                timestamp: 1_700_200_000,
                message: "docs: update readme".to_owned(),
            },
        ]
    }

    #[test]
    fn uniform_cadence_and_messages_detected() {
        let commits = uniform_samples(10, 3_600);
        let metrics = analyze_commits(&commits);
        assert_eq!(metrics.commit_count, 10);
        assert!(
            metrics.cadence_cv < 0.1,
            "identical gaps → CV near zero, got {}",
            metrics.cadence_cv
        );
        assert!(
            metrics.conventional_fraction > 0.9,
            "all conventional messages"
        );
        assert_eq!(metrics.recovery_commit_count, 0);
    }

    #[test]
    fn messy_commits_not_detected() {
        let commits = messy_samples();
        let metrics = analyze_commits(&commits);
        assert!(
            metrics.recovery_commit_count > 0,
            "messy commits should have recovery markers"
        );
        assert!(
            metrics.cadence_cv > 0.3,
            "irregular gaps → high CV: {}",
            metrics.cadence_cv
        );
    }

    #[test]
    fn below_min_commit_count_skipped() {
        let commits = uniform_samples(3, 3_600);
        let config = CommitPatternConfig::default();
        // Not a repo path test — just verify metrics function returns low count
        let metrics = analyze_commits(&commits);
        assert!(metrics.commit_count < config.min_commit_count);
    }

    #[test]
    fn empty_commits_returns_zero_metrics() {
        let metrics = analyze_commits(&[]);
        assert_eq!(metrics.commit_count, 0);
        assert_eq!(metrics.cadence_cv, 0.0);
    }

    #[test]
    fn conventional_prefix_detection() {
        assert!(has_conventional_prefix("feat: add thing"));
        assert!(has_conventional_prefix("fix(auth): handle error"));
        assert!(has_conventional_prefix("refactor!: breaking change"));
        assert!(!has_conventional_prefix("WIP some hack"));
        assert!(!has_conventional_prefix("oops forgot file"));
    }

    #[test]
    fn detect_repo_on_invalid_path_returns_error() {
        let result = detect_repo_with_config("/nonexistent/path", CommitPatternConfig::default());
        assert!(result.is_err(), "invalid repo path should error");
    }
}
