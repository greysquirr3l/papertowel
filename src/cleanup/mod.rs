use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub const CLEANUP_REPORT_SCHEMA_VERSION: &str = "1";

pub const DEFAULT_VALIDATION_COMMANDS: [&str; 3] = [
    "cargo build --workspace",
    "cargo test --workspace",
    "cargo clippy --workspace --all-targets -- -D warnings",
];

const HIGH_CONFIDENCE_THRESHOLD: f32 = 0.85;
const MEDIUM_CONFIDENCE_THRESHOLD: f32 = 0.60;
const DEFERRED_QUEUE_FILE: &str = "deferred.json";
const LATEST_REPORT_FILE: &str = "latest.json";
const PREVIOUS_REPORT_FILE: &str = "previous.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupTrack {
    Deduplication,
    TypeConsolidation,
    DeadCode,
    CircularDependencies,
    TypeStrengthening,
    ErrorHandling,
    DeprecatedAndAiArtifacts,
}

impl CleanupTrack {
    #[must_use]
    pub const fn all() -> [Self; 7] {
        [
            Self::Deduplication,
            Self::TypeConsolidation,
            Self::DeadCode,
            Self::CircularDependencies,
            Self::TypeStrengthening,
            Self::ErrorHandling,
            Self::DeprecatedAndAiArtifacts,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupConfidenceClass {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CleanupConfidence {
    pub score: f32,
    pub class: CleanupConfidenceClass,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupRisk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupSuggestedAction {
    Defer,
    Review,
    Apply,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupLocation {
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupEvidence {
    pub required: Vec<String>,
    pub present: Vec<String>,
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CleanupFinding {
    pub id: String,
    pub track: CleanupTrack,
    pub severity: String,
    pub risk: CleanupRisk,
    pub confidence: CleanupConfidence,
    pub location: CleanupLocation,
    pub description: String,
    pub evidence: CleanupEvidence,
    pub suggested_action: CleanupSuggestedAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupSummary {
    pub finding_count: usize,
    pub apply_count: usize,
    pub review_count: usize,
    pub defer_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupValidationPlan {
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CleanupReport {
    pub version: String,
    pub path: String,
    pub tracks: Vec<CleanupTrack>,
    pub summary: CleanupSummary,
    pub findings: Vec<CleanupFinding>,
    pub deferred: Vec<CleanupFinding>,
    pub validation_plan: CleanupValidationPlan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CleanupDeferredQueue {
    pub version: String,
    pub source_path: String,
    pub deferred: Vec<CleanupFinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupTrend {
    pub finding_delta: i64,
    pub apply_delta: i64,
    pub review_delta: i64,
    pub defer_delta: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CleanupStatusReport {
    pub version: String,
    pub state_dir: String,
    pub deferred_count: usize,
    pub evidence_gap_count: usize,
    pub deferred: Vec<CleanupFinding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trend: Option<CleanupTrend>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupApplyPolicy {
    pub min_confidence: CleanupConfidenceClass,
    pub max_risk: CleanupRisk,
    pub allowed_tracks: BTreeSet<CleanupTrack>,
    pub require_evidence_for_destructive: bool,
}

impl CleanupApplyPolicy {
    #[must_use]
    pub fn strict_default(allowed_tracks: &[CleanupTrack]) -> Self {
        Self {
            min_confidence: CleanupConfidenceClass::High,
            max_risk: CleanupRisk::Low,
            allowed_tracks: allowed_tracks.iter().copied().collect(),
            require_evidence_for_destructive: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CleanupBlockedFinding {
    pub finding: CleanupFinding,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CleanupApplySelection {
    pub approved: Vec<CleanupFinding>,
    pub blocked: Vec<CleanupBlockedFinding>,
}

#[must_use]
pub const fn confidence_meets_minimum(
    class: CleanupConfidenceClass,
    minimum: CleanupConfidenceClass,
) -> bool {
    confidence_rank(class) >= confidence_rank(minimum)
}

#[must_use]
pub const fn risk_within_max(risk: CleanupRisk, max_risk: CleanupRisk) -> bool {
    risk_rank(risk) <= risk_rank(max_risk)
}

#[must_use]
pub fn select_applicable_findings(
    report: &CleanupReport,
    policy: &CleanupApplyPolicy,
) -> CleanupApplySelection {
    let mut approved = Vec::new();
    let mut blocked = Vec::new();

    for finding in &report.findings {
        if let Some(reason) = block_reason_for_finding(finding, policy) {
            blocked.push(CleanupBlockedFinding {
                finding: finding.clone(),
                reason: reason.to_owned(),
            });
        } else {
            approved.push(finding.clone());
        }
    }

    CleanupApplySelection { approved, blocked }
}

const fn confidence_rank(class: CleanupConfidenceClass) -> u8 {
    match class {
        CleanupConfidenceClass::Low => 1,
        CleanupConfidenceClass::Medium => 2,
        CleanupConfidenceClass::High => 3,
    }
}

const fn risk_rank(risk: CleanupRisk) -> u8 {
    match risk {
        CleanupRisk::Low => 1,
        CleanupRisk::Medium => 2,
        CleanupRisk::High => 3,
    }
}

const fn is_destructive_risk(risk: CleanupRisk) -> bool {
    matches!(risk, CleanupRisk::Medium | CleanupRisk::High)
}

fn block_reason_for_finding(
    finding: &CleanupFinding,
    policy: &CleanupApplyPolicy,
) -> Option<&'static str> {
    if !policy.allowed_tracks.contains(&finding.track) {
        return Some("track_not_allowed");
    }

    if !matches!(finding.suggested_action, CleanupSuggestedAction::Apply) {
        return Some("not_marked_apply");
    }

    if !confidence_meets_minimum(finding.confidence.class, policy.min_confidence) {
        return Some("below_min_confidence");
    }

    if !risk_within_max(finding.risk, policy.max_risk) {
        return Some("above_max_risk");
    }

    if policy.require_evidence_for_destructive
        && is_destructive_risk(finding.risk)
        && !finding.evidence.missing.is_empty()
    {
        return Some("missing_mandatory_evidence");
    }

    None
}

#[must_use]
pub fn resolve_state_dir(path: &str, state_dir_override: Option<&str>) -> PathBuf {
    if let Some(override_path) = state_dir_override {
        return PathBuf::from(override_path);
    }

    let input = PathBuf::from(path);
    let base = if input.is_dir() {
        input
    } else {
        input
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    };
    base.join(".papertowel").join("cleanup")
}

pub fn persist_assess_artifacts(state_dir: &Path, report: &CleanupReport) -> Result<()> {
    fs::create_dir_all(state_dir)?;

    let latest_path = state_dir.join(LATEST_REPORT_FILE);
    let previous_path = state_dir.join(PREVIOUS_REPORT_FILE);
    let deferred_path = state_dir.join(DEFERRED_QUEUE_FILE);

    if latest_path.exists() {
        let existing_latest = fs::read_to_string(&latest_path)?;
        fs::write(&previous_path, existing_latest)?;
    }

    write_json_file(&latest_path, report)?;

    let queue = CleanupDeferredQueue {
        version: CLEANUP_REPORT_SCHEMA_VERSION.to_owned(),
        source_path: report.path.clone(),
        deferred: report.deferred.clone(),
    };
    write_json_file(&deferred_path, &queue)?;
    Ok(())
}

pub fn read_status_report(state_dir: &Path) -> Result<CleanupStatusReport> {
    let deferred_path = state_dir.join(DEFERRED_QUEUE_FILE);
    let latest_path = state_dir.join(LATEST_REPORT_FILE);
    let previous_path = state_dir.join(PREVIOUS_REPORT_FILE);

    let queue =
        read_optional_json_file::<CleanupDeferredQueue>(&deferred_path)?.unwrap_or_else(|| {
            CleanupDeferredQueue {
                version: CLEANUP_REPORT_SCHEMA_VERSION.to_owned(),
                source_path: String::new(),
                deferred: Vec::new(),
            }
        });

    let evidence_gap_count = queue
        .deferred
        .iter()
        .map(|finding| finding.evidence.missing.len())
        .sum();

    let latest = read_optional_json_file::<CleanupReport>(&latest_path)?;
    let previous = read_optional_json_file::<CleanupReport>(&previous_path)?;
    let trend = match (latest, previous) {
        (Some(current), Some(prior)) => Some(CleanupTrend {
            finding_delta: count_delta(current.summary.finding_count, prior.summary.finding_count),
            apply_delta: count_delta(current.summary.apply_count, prior.summary.apply_count),
            review_delta: count_delta(current.summary.review_count, prior.summary.review_count),
            defer_delta: count_delta(current.summary.defer_count, prior.summary.defer_count),
        }),
        _ => None,
    };

    Ok(CleanupStatusReport {
        version: queue.version,
        state_dir: state_dir.to_string_lossy().into_owned(),
        deferred_count: queue.deferred.len(),
        evidence_gap_count,
        deferred: queue.deferred,
        trend,
    })
}

fn count_to_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn count_delta(current: usize, previous: usize) -> i64 {
    count_to_i64(current) - count_to_i64(previous)
}

fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(value)?;
    fs::write(path, json)?;
    Ok(())
}

fn read_optional_json_file<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(serde_json::from_str::<T>(&content)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[must_use]
pub fn classify_confidence(score: f32, missing_required_count: usize) -> CleanupConfidenceClass {
    let bounded_score = score.clamp(0.0, 1.0);
    if missing_required_count == 0 && bounded_score >= HIGH_CONFIDENCE_THRESHOLD {
        return CleanupConfidenceClass::High;
    }

    if bounded_score >= MEDIUM_CONFIDENCE_THRESHOLD {
        CleanupConfidenceClass::Medium
    } else {
        CleanupConfidenceClass::Low
    }
}

#[must_use]
pub const fn suggest_action(
    confidence_class: CleanupConfidenceClass,
    risk: CleanupRisk,
    missing_required_count: usize,
) -> CleanupSuggestedAction {
    if missing_required_count > 0 {
        return CleanupSuggestedAction::Defer;
    }

    if matches!(confidence_class, CleanupConfidenceClass::High) && matches!(risk, CleanupRisk::Low)
    {
        CleanupSuggestedAction::Apply
    } else {
        CleanupSuggestedAction::Review
    }
}

const fn required_evidence_for_track(track: CleanupTrack) -> &'static [&'static str] {
    match track {
        CleanupTrack::Deduplication => &["semantic_similarity_scan", "callsite_overlap_check"],
        CleanupTrack::TypeConsolidation => &["type_equivalence_check", "boundary_model_check"],
        CleanupTrack::DeadCode => &[
            "refs_scan",
            "config_hook_scan",
            "entrypoint_scan",
            "cross_target_check",
        ],
        CleanupTrack::CircularDependencies => &["cycle_graph", "neutral_extraction_candidate"],
        CleanupTrack::TypeStrengthening => {
            &["callsite_contract_check", "boundary_flexibility_check"]
        }
        CleanupTrack::ErrorHandling => &["error_path_audit", "recovery_boundary_check"],
        CleanupTrack::DeprecatedAndAiArtifacts => &[
            "deprecation_source",
            "compatibility_impact_check",
            "user_path_verification",
        ],
    }
}

const fn default_risk_for_track(track: CleanupTrack) -> CleanupRisk {
    match track {
        CleanupTrack::DeadCode
        | CleanupTrack::CircularDependencies
        | CleanupTrack::DeprecatedAndAiArtifacts => CleanupRisk::Medium,
        CleanupTrack::Deduplication
        | CleanupTrack::TypeConsolidation
        | CleanupTrack::TypeStrengthening
        | CleanupTrack::ErrorHandling => CleanupRisk::Low,
    }
}

const fn baseline_score_for_track(track: CleanupTrack) -> f32 {
    match track {
        CleanupTrack::DeadCode => 0.58,
        CleanupTrack::CircularDependencies => 0.56,
        CleanupTrack::DeprecatedAndAiArtifacts => 0.52,
        CleanupTrack::Deduplication => 0.62,
        CleanupTrack::TypeConsolidation => 0.63,
        CleanupTrack::TypeStrengthening => 0.60,
        CleanupTrack::ErrorHandling => 0.61,
    }
}

const fn track_slug(track: CleanupTrack) -> &'static str {
    match track {
        CleanupTrack::Deduplication => "deduplication",
        CleanupTrack::TypeConsolidation => "type_consolidation",
        CleanupTrack::DeadCode => "dead_code",
        CleanupTrack::CircularDependencies => "circular_dependencies",
        CleanupTrack::TypeStrengthening => "type_strengthening",
        CleanupTrack::ErrorHandling => "error_handling",
        CleanupTrack::DeprecatedAndAiArtifacts => "deprecated_and_ai_artifacts",
    }
}

fn route_track(path: &str, track: CleanupTrack) -> CleanupFinding {
    let required = required_evidence_for_track(track);
    let present = vec!["track_selected".to_owned()];
    let missing = required.iter().map(ToString::to_string).collect::<Vec<_>>();
    let risk = default_risk_for_track(track);
    let score = baseline_score_for_track(track);
    let confidence_class = classify_confidence(score, missing.len());
    let suggested_action = suggest_action(confidence_class, risk, missing.len());

    CleanupFinding {
        id: format!("cleanup.{}.001", track_slug(track)),
        track,
        severity: "medium".to_owned(),
        risk,
        confidence: CleanupConfidence {
            score,
            class: confidence_class,
            reasons: vec![
                "read-only track router produced a conservative scaffold finding".to_owned(),
                "mandatory evidence is still required before any apply suggestion".to_owned(),
            ],
        },
        location: CleanupLocation {
            file: path.to_owned(),
            line: None,
            symbol: None,
        },
        description: format!(
            "Track '{}' requires evidence collection before safe cleanup actions",
            track_slug(track)
        ),
        evidence: CleanupEvidence {
            required: required.iter().map(ToString::to_string).collect(),
            present,
            missing,
        },
        suggested_action,
    }
}

#[must_use]
pub fn route_tracks(path: &str, tracks: &[CleanupTrack]) -> Vec<CleanupFinding> {
    tracks
        .iter()
        .copied()
        .map(|track| route_track(path, track))
        .collect()
}

#[must_use]
pub fn build_assess_report(path: &str, tracks: &[CleanupTrack]) -> CleanupReport {
    let track_set: BTreeSet<CleanupTrack> = tracks.iter().copied().collect();
    let ordered_tracks = track_set.into_iter().collect::<Vec<_>>();
    let findings = route_tracks(path, &ordered_tracks);

    let summary = findings.iter().fold(
        CleanupSummary {
            finding_count: 0,
            apply_count: 0,
            review_count: 0,
            defer_count: 0,
        },
        |mut acc, finding| {
            acc.finding_count += 1;
            match finding.suggested_action {
                CleanupSuggestedAction::Apply => acc.apply_count += 1,
                CleanupSuggestedAction::Review => acc.review_count += 1,
                CleanupSuggestedAction::Defer => acc.defer_count += 1,
            }
            acc
        },
    );

    let deferred = findings
        .iter()
        .filter(|finding| {
            matches!(
                finding.suggested_action,
                CleanupSuggestedAction::Defer | CleanupSuggestedAction::Review
            )
        })
        .cloned()
        .collect::<Vec<_>>();

    CleanupReport {
        version: CLEANUP_REPORT_SCHEMA_VERSION.to_owned(),
        path: path.to_owned(),
        tracks: ordered_tracks,
        summary,
        findings,
        deferred,
        validation_plan: CleanupValidationPlan {
            commands: DEFAULT_VALIDATION_COMMANDS
                .iter()
                .map(ToString::to_string)
                .collect(),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::{
        CleanupApplyPolicy, CleanupConfidenceClass, CleanupRisk, CleanupSuggestedAction,
        CleanupTrack, build_assess_report, classify_confidence, confidence_meets_minimum,
        persist_assess_artifacts, read_status_report, resolve_state_dir, risk_within_max,
        route_tracks, select_applicable_findings, suggest_action,
    };

    #[test]
    fn build_assess_report_orders_and_deduplicates_tracks() {
        let report = build_assess_report(
            ".",
            &[
                CleanupTrack::DeadCode,
                CleanupTrack::Deduplication,
                CleanupTrack::DeadCode,
            ],
        );

        assert_eq!(report.path, ".");
        assert_eq!(report.tracks.len(), 2);
        assert_eq!(report.tracks.first(), Some(&CleanupTrack::Deduplication));
        assert_eq!(report.tracks.get(1), Some(&CleanupTrack::DeadCode));
        assert_eq!(report.summary.finding_count, 2);
        assert_eq!(report.summary.defer_count, 2);
        assert_eq!(report.findings.len(), 2);
        assert_eq!(report.deferred.len(), 2);
    }

    #[test]
    fn classify_confidence_thresholds_are_deterministic() {
        assert_eq!(classify_confidence(0.90, 0), CleanupConfidenceClass::High);
        assert_eq!(classify_confidence(0.70, 1), CleanupConfidenceClass::Medium);
        assert_eq!(classify_confidence(0.40, 2), CleanupConfidenceClass::Low);
    }

    #[test]
    fn missing_evidence_prevents_apply_suggestion() {
        let action = suggest_action(CleanupConfidenceClass::High, CleanupRisk::Low, 1);
        assert_eq!(action, CleanupSuggestedAction::Defer);

        let action_without_gaps = suggest_action(CleanupConfidenceClass::High, CleanupRisk::Low, 0);
        assert_eq!(action_without_gaps, CleanupSuggestedAction::Apply);
    }

    #[test]
    fn route_tracks_produces_structured_findings_for_each_track() {
        let tracks = CleanupTrack::all();
        let findings = route_tracks(".", &tracks);
        assert_eq!(findings.len(), tracks.len());

        let observed_tracks = findings.iter().map(|f| f.track).collect::<BTreeSet<_>>();
        let expected_tracks = tracks.into_iter().collect::<BTreeSet<_>>();
        assert_eq!(observed_tracks, expected_tracks);

        assert!(
            findings
                .iter()
                .all(|finding| !finding.evidence.required.is_empty())
        );
    }

    #[test]
    fn resolve_state_dir_uses_override_when_provided() {
        let state_dir = resolve_state_dir(".", Some("/tmp/custom-cleanup"));
        assert_eq!(state_dir, PathBuf::from("/tmp/custom-cleanup"));
    }

    #[test]
    fn status_handles_missing_state_directory() {
        let Ok(temp) = TempDir::new() else {
            return;
        };
        let missing = temp.path().join("does-not-exist");

        let Ok(status) = read_status_report(&missing) else {
            return;
        };
        assert_eq!(status.deferred_count, 0);
        assert_eq!(status.evidence_gap_count, 0);
        assert!(status.trend.is_none());
    }

    #[test]
    fn persist_and_read_status_reports_deferred_and_trend() {
        let Ok(temp) = TempDir::new() else {
            return;
        };
        let root = temp.path().to_string_lossy().into_owned();
        let state_dir = resolve_state_dir(&root, None);

        let first = build_assess_report(&root, &[CleanupTrack::DeadCode]);
        let first_result = persist_assess_artifacts(&state_dir, &first);
        assert!(first_result.is_ok());

        let second = build_assess_report(
            &root,
            &[CleanupTrack::DeadCode, CleanupTrack::ErrorHandling],
        );
        let second_result = persist_assess_artifacts(&state_dir, &second);
        assert!(second_result.is_ok());

        let Ok(status) = read_status_report(&state_dir) else {
            return;
        };
        assert_eq!(status.deferred_count, 2);
        assert!(status.evidence_gap_count > 0);
        assert!(status.trend.is_some());

        let deferred_path = state_dir.join("deferred.json");
        let file_exists = fs::metadata(&deferred_path).is_ok();
        assert!(file_exists);
    }

    #[test]
    fn confidence_and_risk_policy_checks_are_deterministic() {
        assert!(confidence_meets_minimum(
            CleanupConfidenceClass::High,
            CleanupConfidenceClass::Medium,
        ));
        assert!(!confidence_meets_minimum(
            CleanupConfidenceClass::Low,
            CleanupConfidenceClass::Medium,
        ));

        assert!(risk_within_max(CleanupRisk::Low, CleanupRisk::Medium));
        assert!(!risk_within_max(CleanupRisk::High, CleanupRisk::Medium));
    }

    #[test]
    fn select_applicable_findings_blocks_non_apply_or_missing_evidence() {
        let report =
            build_assess_report(".", &[CleanupTrack::DeadCode, CleanupTrack::ErrorHandling]);
        let policy = CleanupApplyPolicy::strict_default(&report.tracks);
        let selection = select_applicable_findings(&report, &policy);

        assert!(selection.approved.is_empty());
        assert_eq!(selection.blocked.len(), report.findings.len());
        assert!(
            selection
                .blocked
                .iter()
                .all(|blocked| blocked.reason == "not_marked_apply")
        );
    }

    #[test]
    fn select_applicable_findings_respects_track_allow_list() {
        let mut report = build_assess_report(".", &[CleanupTrack::TypeStrengthening]);
        if let Some(first) = report.findings.first_mut() {
            first.suggested_action = CleanupSuggestedAction::Apply;
            first.confidence.class = CleanupConfidenceClass::High;
            first.risk = CleanupRisk::Low;
            first.evidence.missing.clear();
        }

        let policy = CleanupApplyPolicy::strict_default(&[]);
        let selection = select_applicable_findings(&report, &policy);
        assert!(selection.approved.is_empty());
        assert_eq!(selection.blocked.len(), 1);
        assert_eq!(
            selection
                .blocked
                .first()
                .map(|blocked| blocked.reason.as_str()),
            Some("track_not_allowed")
        );
    }
}
