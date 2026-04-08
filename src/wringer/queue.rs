use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use chrono::{DateTime, Duration, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use git2::{BranchType, Oid, Repository};
use serde::{Deserialize, Serialize};

use crate::domain::errors::PapertowelError;
use crate::profile::persona::PersonaProfile;

pub const COMPONENT_NAME: &str = "queue";

/// The maximum time gap (in seconds) between two commits that are still
/// considered part of the same "session" for grouping purposes.
const SESSION_GAP_SECONDS: i64 = 600;

/// Maximum files-in-common ratio below which two commits are considered
/// unrelated (used to flag split candidates inside a group).
const SPLIT_FILE_OVERLAP_THRESHOLD: usize = 2;

// ─── Domain types ────────────────────────────────────────────────────────────

/// A single commit from the source branch that is pending replay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingCommit {
    pub oid: String,
    pub message: String,
    pub author: String,
    pub timestamp: DateTime<Utc>,
    /// Paths changed by this commit (relative to repo root).
    pub changed_files: Vec<String>,
}

/// Whether a planned entry should be replayed as-is, squashed, or flagged for
/// splitting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayAction {
    /// Replay this commit verbatim (possibly with a new message/timestamp).
    Replay,
    /// Squash this commit with adjacent commits into a single public commit.
    Squash,
    /// This large commit is a candidate for manual splitting before replay.
    Split,
}

/// One entry in the replay plan, corresponding to one or more source commits
/// that will produce one public commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueEntry {
    /// Source commit OIDs that map to this entry (one for Replay, multiple for
    /// Squash).
    pub source_oids: Vec<String>,
    /// The commit message to use for the public commit (may be humanized
    /// later).
    pub message: String,
    /// The target wall-clock time at which `wring drip` should apply this
    /// commit.
    pub target_time: DateTime<Utc>,
    pub action: ReplayAction,
    /// Whether this entry has already been drip-fed into the public worktree.
    pub completed: bool,
}

/// The full queue plan, serialized to `.papertowel/queue.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuePlan {
    /// OID of the last commit already present in the public branch (None if
    /// the public branch is empty).
    pub sync_point: Option<String>,
    /// The persona name used to build this plan.
    pub persona_name: String,
    /// Ordered list of entries to replay.
    pub entries: Vec<QueueEntry>,
    /// UTC timestamp when this plan was generated.
    pub generated_at: DateTime<Utc>,
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Walk `source_branch` since `sync_oid` (exclusive) and return a list of
/// `PendingCommit`s in chronological order.
///
/// If `sync_oid` is `None`, all commits reachable from `source_branch` are
/// returned.
pub fn collect_pending_commits(
    repo_path: impl AsRef<Path>,
    source_branch: &str,
    sync_oid: Option<&str>,
) -> Result<Vec<PendingCommit>, PapertowelError> {
    let repo = Repository::open(repo_path.as_ref())?;

    let branch = repo.find_branch(source_branch, BranchType::Local)?;
    let tip = branch
        .get()
        .peel_to_commit()
        .map_err(PapertowelError::Git)?;

    let stop_oid: Option<Oid> = sync_oid
        .map(|s| Oid::from_str(s).map_err(PapertowelError::Git))
        .transpose()?;

    let mut walker = repo.revwalk()?;
    walker.push(tip.id())?;
    walker.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;

    let mut commits: Vec<PendingCommit> = Vec::new();

    for oid_result in walker {
        let oid = oid_result?;

        if stop_oid.is_some_and(|stop| stop == oid) {
            break;
        }

        let commit = repo.find_commit(oid)?;
        let changed_files = changed_files_for_commit(&repo, &commit)?;
        let timestamp = DateTime::from_timestamp(commit.time().seconds(), 0).unwrap_or_else(Utc::now);

        commits.push(PendingCommit {
            oid: oid.to_string(),
            message: commit.message().unwrap_or("<no message>").trim().to_owned(),
            author: commit.author().name().unwrap_or("unknown").to_owned(),
            timestamp,
            changed_files,
        });
    }

    // Reverse to chronological order (oldest first).
    commits.reverse();
    Ok(commits)
}

/// Build a `QueuePlan` from a list of pending commits and a persona profile.
///
/// The planner:
/// 1. Groups commits into "sessions" by temporal proximity.
/// 2. Inside each session, merges commits with high file overlap into squash
///    groups.
/// 3. Flags very large commits touching many unrelated file trees as split
///    candidates.
/// 4. Assigns target timestamps spread across the persona's active hours.
pub fn build_queue_plan(
    pending: &[PendingCommit],
    persona: &PersonaProfile,
    sync_point: Option<String>,
    now: DateTime<Utc>,
) -> Result<QueuePlan, PapertowelError> {
    let sessions = group_into_sessions(pending);
    let mut entries: Vec<QueueEntry> = Vec::new();

    // Determine persona timezone; fall back to UTC if parsing fails.
    let tz: Tz = persona.timezone.parse().unwrap_or(chrono_tz::UTC);

    // Build active windows from persona schedule.
    let windows = parse_active_windows(&persona.schedule.active_hours, tz);

    // We schedule entries starting from `now`, advancing through persona
    // windows.
    let mut cursor = next_active_time(now, &windows, tz);

    for session_commits in &sessions {
        let groups = squash_groups(session_commits);

        for group in groups {
            let action = classify_action(&group);
            let oids: Vec<String> = group.iter().map(|c| c.oid.clone()).collect();
            let message = group.first().map(|c| c.message.clone()).unwrap_or_default();

            entries.push(QueueEntry {
                source_oids: oids,
                message,
                target_time: cursor,
                action,
                completed: false,
            });

            // Advance cursor by a jitter interval within the session.
            let jitter = jitter_minutes(&persona.schedule);
            cursor = advance_cursor(cursor, jitter, &windows, tz);
        }
    }

    Ok(QueuePlan {
        sync_point,
        persona_name: persona.name.clone(),
        entries,
        generated_at: Utc::now(),
    })
}

/// Persist a `QueuePlan` to `.papertowel/queue.json` inside `repo_path`.
pub fn save_queue_plan(
    repo_path: impl AsRef<Path>,
    plan: &QueuePlan,
) -> Result<(), PapertowelError> {
    let state_dir = repo_path.as_ref().join(".papertowel");
    fs::create_dir_all(&state_dir).map_err(|e| PapertowelError::io_with_path(&state_dir, e))?;

    let path = state_dir.join("queue.json");
    let json = serde_json::to_string_pretty(plan)?;
    fs::write(&path, json).map_err(|e| PapertowelError::io_with_path(&path, e))?;
    Ok(())
}

/// Load a previously saved `QueuePlan` from `.papertowel/queue.json`.
pub fn load_queue_plan(repo_path: impl AsRef<Path>) -> Result<QueuePlan, PapertowelError> {
    let path = repo_path.as_ref().join(".papertowel").join("queue.json");
    let json = fs::read_to_string(&path).map_err(|e| PapertowelError::io_with_path(&path, e))?;
    let plan = serde_json::from_str(&json)?;
    Ok(plan)
}

// ─── Internal helpers ────────────────────────────────────────────────────────

/// Return the list of files changed by a commit relative to its first parent.
fn changed_files_for_commit(
    repo: &Repository,
    commit: &git2::Commit<'_>,
) -> Result<Vec<String>, PapertowelError> {
    let commit_tree = commit.tree()?;
    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());

    let mut diff_opts = git2::DiffOptions::new();
    let diff = repo.diff_tree_to_tree(
        parent_tree.as_ref(),
        Some(&commit_tree),
        Some(&mut diff_opts),
    )?;

    let mut files: Vec<String> = Vec::new();
    diff.foreach(
        &mut |delta, _| {
            if let Some(path) = delta.new_file().path().and_then(|p| p.to_str()) {
                files.push(path.to_owned());
            }
            true
        },
        None,
        None,
        None,
    )?;

    Ok(files)
}

/// Group commits into sessions: any gap larger than `SESSION_GAP_SECONDS`
/// starts a new session.
fn group_into_sessions(commits: &[PendingCommit]) -> Vec<Vec<&PendingCommit>> {
    let mut sessions: Vec<Vec<&PendingCommit>> = Vec::new();
    let mut current: Vec<&PendingCommit> = Vec::new();

    for commit in commits {
        if let Some(prev) = current.last() {
            let gap = (commit.timestamp - prev.timestamp).num_seconds().abs();
            if gap > SESSION_GAP_SECONDS {
                sessions.push(current);
                current = Vec::new();
            }
        }
        current.push(commit);
    }

    if !current.is_empty() {
        sessions.push(current);
    }

    sessions
}

/// Within a session, group adjacent commits that touch at least one common file
/// into squash groups.
fn squash_groups<'a>(session: &[&'a PendingCommit]) -> Vec<Vec<&'a PendingCommit>> {
    let mut groups: Vec<Vec<&PendingCommit>> = Vec::new();
    let mut current: Vec<&PendingCommit> = Vec::new();

    for commit in session {
        if current.is_empty() {
            current.push(commit);
            continue;
        }

        let current_files: HashSet<&str> = current
            .iter()
            .flat_map(|c| c.changed_files.iter().map(String::as_str))
            .collect();
        let this_files: HashSet<&str> = commit.changed_files.iter().map(String::as_str).collect();
        let overlap = current_files.intersection(&this_files).count();

        if overlap > 0 {
            current.push(commit);
        } else {
            groups.push(current);
            current = vec![commit];
        }
    }

    if !current.is_empty() {
        groups.push(current);
    }

    groups
}

/// Decide the replay action for a group of commits.
///
/// - Single commit touching many unrelated module roots → `Split`
/// - Multiple commits in a group → `Squash`
/// - Everything else → `Replay`
fn classify_action(group: &[&PendingCommit]) -> ReplayAction {
    if group.len() == 1 {
        let Some(commit) = group.first() else {
            return ReplayAction::Replay;
        };
        // Count distinct top-level paths (module roots).
        let roots: HashSet<&str> = commit
            .changed_files
            .iter()
            .filter_map(|f| f.split('/').next())
            .collect();
        if roots.len() > SPLIT_FILE_OVERLAP_THRESHOLD {
            return ReplayAction::Split;
        }
        return ReplayAction::Replay;
    }

    ReplayAction::Squash
}

/// A parsed active-hours window: (start, end) in naive local time.
struct ActiveWindow {
    start: NaiveTime,
    end: NaiveTime,
    /// Whether the window wraps midnight (e.g., "22:00-02:00").
    wraps: bool,
}

/// Parse `"HH:MM-HH:MM"` strings into `ActiveWindow`s.
fn parse_active_windows(windows: &[String], _tz: Tz) -> Vec<ActiveWindow> {
    windows
        .iter()
        .filter_map(|s| {
            let (start_str, end_str) = s.split_once('-')?;
            let start = parse_naive_time(start_str)?;
            let end = parse_naive_time(end_str)?;
            let wraps = end <= start;
            Some(ActiveWindow { start, end, wraps })
        })
        .collect()
}

fn parse_naive_time(s: &str) -> Option<NaiveTime> {
    let (h, m) = s.split_once(':')?;
    let hour: u32 = h.parse().ok()?;
    let minute: u32 = m.parse().ok()?;
    NaiveTime::from_hms_opt(hour, minute, 0)
}

/// Find the next moment >= `from` that falls inside any of the persona's active
/// windows. Falls back to `from` + 1 hour if no windows are defined.
fn next_active_time(from: DateTime<Utc>, windows: &[ActiveWindow], tz: Tz) -> DateTime<Utc> {
    if windows.is_empty() {
        return from + Duration::hours(1);
    }

    let local = from.with_timezone(&tz);
    let local_time = local.time();

    for window in windows {
        if time_in_window(local_time, window) {
            return from;
        }
    }

    // Find the nearest upcoming window start.
    let mut best: Option<DateTime<Utc>> = None;
    for window in windows {
        let candidate_local = local.date_naive().and_time(window.start);
        let candidate_utc = tz
            .from_local_datetime(&candidate_local)
            .single()
            .map(|dt| dt.with_timezone(&Utc));

        if let Some(candidate) = candidate_utc {
            let candidate_adjusted = if candidate <= from {
                candidate + Duration::days(1)
            } else {
                candidate
            };

            if best.is_none_or(|b| candidate_adjusted < b) {
                best = Some(candidate_adjusted);
            }
        }
    }

    best.unwrap_or_else(|| from + Duration::hours(1))
}

fn time_in_window(t: NaiveTime, window: &ActiveWindow) -> bool {
    if window.wraps {
        t >= window.start || t < window.end
    } else {
        t >= window.start && t < window.end
    }
}

/// Compute a jitter duration in minutes based on persona session variance.
/// Returns a `Duration` between `avg/2` and `avg * (1 + variance)`.
#[expect(clippy::cast_possible_truncation, reason = "jitter is bounded by session minutes, fits i64")]
fn jitter_minutes(schedule: &crate::profile::persona::PersonaSchedule) -> Duration {
    let avg = i64::from(schedule.avg_commits_per_session).max(1);
    // Spread commits roughly evenly across ~2 hour sessions.
    let session_minutes: i64 = 120;
    let per_commit_minutes = session_minutes / avg;
    // Simple deterministic jitter: vary by ±25% of per-commit interval.
    let variance = (f64::from(schedule.session_variance)
        * f64::from(i32::try_from(per_commit_minutes).unwrap_or(15)))
    .round() as i64;
    let jitter = variance.max(1);
    Duration::minutes(per_commit_minutes + jitter)
}

/// Advance cursor by `interval`, staying within active windows where possible.
fn advance_cursor(
    cursor: DateTime<Utc>,
    interval: Duration,
    windows: &[ActiveWindow],
    tz: Tz,
) -> DateTime<Utc> {
    let next = cursor + interval;
    next_active_time(next, windows, tz)
}

/// Return a map of file path → count of commits touching that file, for
/// informational reporting.
pub fn file_touch_counts(pending: &[PendingCommit]) -> HashMap<String, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for commit in pending {
        for file in &commit.changed_files {
            *counts.entry(file.clone()).or_insert(0) += 1;
        }
    }
    counts
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::Utc;
    use git2::{IndexAddOption, Repository, Signature};
    use tempfile::TempDir;

    use super::{
        QueuePlan, ReplayAction, build_queue_plan, collect_pending_commits, file_touch_counts,
        load_queue_plan, save_queue_plan,
    };
    use crate::profile::persona::PersonaProfile;

    /// Create a minimal repository with one commit and return (`TempDir`,
    /// `repo_path`, `default_branch_name`).
    fn make_repo_with_commit(
        msg: &str,
        file: &str,
    ) -> Result<(TempDir, std::path::PathBuf, String), Box<dyn std::error::Error>> {
        let tmp = TempDir::new()?;
        let repo_path = tmp.path().join("repo");
        fs::create_dir_all(&repo_path)?;

        let repo = Repository::init(&repo_path)?;
        let file_path = repo_path.join(file);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(file_path, "content\n")?;

        let mut index = repo.index()?;
        index.add_all(std::iter::once(&"*"), IndexAddOption::DEFAULT, None)?;
        index.write()?;

        let tree_oid = index.write_tree()?;
        let tree = repo.find_tree(tree_oid)?;
        let sig = Signature::now("test", "test@example.com")?;
        repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &[])?;

        let head_ref = repo.head()?;
        let branch_name = head_ref.shorthand().unwrap_or("main").to_owned();

        Ok((tmp, repo_path, branch_name))
    }

    #[test]
    fn collect_pending_commits_returns_commits_from_branch() -> Result<(), Box<dyn std::error::Error>> {
        let (tmp, repo_path, branch_name) = make_repo_with_commit("initial commit", "README.md")?;
        let _keep = &tmp;

        let commits = collect_pending_commits(&repo_path, &branch_name, None)?;
        assert!(!commits.is_empty(), "expected at least one commit");
        let first = commits.first().ok_or("expected at least one commit")?;
        assert_eq!(first.message, "initial commit");
        Ok(())
    }

    #[test]
    fn build_queue_plan_produces_entries_for_pending_commits() -> Result<(), Box<dyn std::error::Error>> {
        let (tmp, repo_path, branch_name) = make_repo_with_commit("add feature", "src/lib.rs")?;
        let _keep = &tmp;

        let pending = collect_pending_commits(&repo_path, &branch_name, None)?;
        let profiles = PersonaProfile::built_in_profiles();
        let persona = profiles.first().ok_or("no built-in profiles")?; // night-owl

        let now = Utc::now();
        let plan = build_queue_plan(&pending, persona, None, now)?;

        assert!(!plan.entries.is_empty());
        assert_eq!(plan.persona_name, persona.name);
        Ok(())
    }

    #[test]
    fn queue_plan_roundtrips_json() -> Result<(), Box<dyn std::error::Error>> {
        let (tmp, repo_path, branch_name) = make_repo_with_commit("feat: something", "main.rs")?;
        let _keep = &tmp;

        let pending = collect_pending_commits(&repo_path, &branch_name, None)?;
        let profiles = PersonaProfile::built_in_profiles();
        let persona = profiles.get(1).ok_or("expected nine-to-five profile")?; // nine-to-five

        let now = Utc::now();
        let plan = build_queue_plan(&pending, persona, None, now)?;

        save_queue_plan(&repo_path, &plan)?;

        let loaded: QueuePlan = load_queue_plan(&repo_path)?;
        assert_eq!(loaded.persona_name, plan.persona_name);
        assert_eq!(loaded.entries.len(), plan.entries.len());
        Ok(())
    }

    #[test]
    fn file_touch_counts_accumulates_per_file() {
        let commits = vec![
            super::PendingCommit {
                oid: "aaa".to_owned(),
                message: "one".to_owned(),
                author: "x".to_owned(),
                timestamp: Utc::now(),
                changed_files: vec!["src/lib.rs".to_owned(), "README.md".to_owned()],
            },
            super::PendingCommit {
                oid: "bbb".to_owned(),
                message: "two".to_owned(),
                author: "x".to_owned(),
                timestamp: Utc::now(),
                changed_files: vec!["src/lib.rs".to_owned()],
            },
        ];
        let counts = file_touch_counts(&commits);
        assert_eq!(counts.get("src/lib.rs").copied(), Some(2));
        assert_eq!(counts.get("README.md").copied(), Some(1));
    }

    #[test]
    fn replay_action_classifies_single_module_as_replay() {
        use super::{PendingCommit, classify_action};

        let commit = PendingCommit {
            oid: "abc".to_owned(),
            message: "fix".to_owned(),
            author: "dev".to_owned(),
            timestamp: Utc::now(),
            changed_files: vec!["src/foo.rs".to_owned(), "src/bar.rs".to_owned()],
        };
        let group = vec![&commit];
        // Both files are under "src" → 1 root → Replay
        assert_eq!(classify_action(&group), ReplayAction::Replay);
    }
}
