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
const SESSION_GAP_SECONDS: i64 = 600;

/// Maximum files-in-common ratio below which two commits are considered
const SPLIT_FILE_OVERLAP_THRESHOLD: usize = 2;

// ─── Domain types ────────────────────────────────────────────────────────────

/// A single commit from the source branch that is pending replay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingCommit {
 pub oid: String,
 pub message: String,
 pub author: String,
 pub timestamp: DateTime<Utc>,
 pub changed_files: Vec<String>,
}

/// splitting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayAction {
 /// Replay this commit verbatim (possibly with a new message/timestamp).
 Replay,
 Squash,
 Split,
}

/// that will produce one public commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueEntry {
 /// Squash).
 pub source_oids: Vec<String>,
 /// later).
 pub message: String,
 /// The target wall-clock time at which `wring drip` should apply this
 /// commit.
 pub target_time: DateTime<Utc>,
 pub action: ReplayAction,
 pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuePlan {
 /// OID of the last commit already present in the public branch (None if
 /// the public branch is empty).
 pub sync_point: Option<String>,
 pub persona_name: String,
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
 let timestamp =
 DateTime::from_timestamp(commit.time().seconds(), 0).unwrap_or_else(Utc::now);

 commits.push(PendingCommit {
 oid: oid.to_string(),
 message: commit.message().unwrap_or("<no message>").trim().to_owned(),
 author: commit.author().name().unwrap_or("unknown").to_owned(),
 timestamp,
 changed_files,
 });
 }

 commits.reverse();
 Ok(commits)
}

/// Build a `QueuePlan` from a list of pending commits and a persona profile.
///
/// The planner:
/// groups.
/// candidates.
/// 4. Assigns target timestamps spread across the persona's active hours.
pub fn build_queue_plan(
 pending: &[PendingCommit],
 persona: &PersonaProfile,
 sync_point: Option<String>,
 now: DateTime<Utc>,
) -> Result<QueuePlan, PapertowelError> {
 let sessions = group_into_sessions(pending);
 let mut entries: Vec<QueueEntry> = Vec::new();

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

pub fn load_queue_plan(repo_path: impl AsRef<Path>) -> Result<QueuePlan, PapertowelError> {
 let path = repo_path.as_ref().join(".papertowel").join("queue.json");
 let json = fs::read_to_string(&path).map_err(|e| PapertowelError::io_with_path(&path, e))?;
 let plan = serde_json::from_str(&json)?;
 Ok(plan)
}

// ─── Internal helpers ────────────────────────────────────────────────────────

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

 if!current.is_empty() {
 sessions.push(current);
 }

 sessions
}

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

 if!current.is_empty() {
 groups.push(current);
 }

 groups
}

///
/// - Multiple commits in a group → `Squash`
/// - Everything else → `Replay`
fn classify_action(group: &[&PendingCommit]) -> ReplayAction {
 if group.len() == 1 {
 let Some(commit) = group.first() else {
 return ReplayAction::Replay;
 };
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
#[expect(
 clippy::cast_possible_truncation,
 reason = "jitter is bounded by session minutes, fits i64"
)]
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
 #![expect(
 clippy::indexing_slicing,
 reason = "indexed assertions on known-populated vecs"
 )]

 use std::fs;

 use chrono::Utc;
 use git2::{IndexAddOption, Repository, Signature};
 use tempfile::TempDir;

 use super::{
 QueuePlan, ReplayAction, build_queue_plan, collect_pending_commits, file_touch_counts,
 load_queue_plan, save_queue_plan,
 };
 use crate::profile::persona::PersonaProfile;

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
 fn collect_pending_commits_returns_commits_from_branch()
 -> Result<(), Box<dyn std::error::Error>> {
 let (tmp, repo_path, branch_name) = make_repo_with_commit("initial commit", "README.md")?;
 let _keep = &tmp;

 let commits = collect_pending_commits(&repo_path, &branch_name, None)?;
 assert!(!commits.is_empty(), "expected at least one commit");
 let first = commits.first().ok_or("expected at least one commit")?;
 assert_eq!(first.message, "initial commit");
 Ok(())
 }

 #[test]
 fn build_queue_plan_produces_entries_for_pending_commits()
 -> Result<(), Box<dyn std::error::Error>> {
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
 fn collect_pending_commits_stops_at_sync_oid() -> Result<(), Box<dyn std::error::Error>> {
 let tmp = TempDir::new()?;
 let repo_path = tmp.path().join("repo");
 fs::create_dir_all(&repo_path)?;

 let repo = Repository::init(&repo_path)?;
 let sig = Signature::now("test", "test@example.com")?;

 // First commit
 fs::write(repo_path.join("a.rs"), "first\n")?;
 let mut index = repo.index()?;
 index.add_all(std::iter::once(&"*"), IndexAddOption::DEFAULT, None)?;
 index.write()?;
 let tree1 = repo.find_tree(index.write_tree()?)?;
 let first_oid = repo.commit(Some("HEAD"), &sig, &sig, "first commit", &tree1, &[])?;

 fs::write(repo_path.join("b.rs"), "second\n")?;
 let mut index = repo.index()?;
 index.add_all(std::iter::once(&"*"), IndexAddOption::DEFAULT, None)?;
 index.write()?;
 let tree2 = repo.find_tree(index.write_tree()?)?;
 let parent = repo.find_commit(first_oid)?;
 repo.commit(
 Some("HEAD"),
 &sig,
 &sig,
 "second commit",
 &tree2,
 &[&parent],
 )?;

 let head_ref = repo.head()?;
 let branch_name = head_ref.shorthand().unwrap_or("main").to_owned();
 let stop_oid_str = first_oid.to_string();

 let commits = collect_pending_commits(&repo_path, &branch_name, Some(&stop_oid_str))?;
 assert_eq!(commits.len(), 1, "should stop before first commit");
 assert_eq!(commits[0].message, "second commit");
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

 #[test]
 fn replay_action_classifies_many_roots_as_split() {
 use super::{PendingCommit, classify_action};

 let commit = PendingCommit {
 oid: "def".to_owned(),
 message: "big change".to_owned(),
 author: "dev".to_owned(),
 timestamp: Utc::now(),
 changed_files: vec![
 "src/foo.rs".to_owned(),
 "docs/guide.md".to_owned(),
 "tests/integration.rs".to_owned(),
 ],
 };
 let group = vec![&commit];
 assert_eq!(classify_action(&group), ReplayAction::Split);
 }

 #[test]
 fn replay_action_classifies_multi_commit_group_as_squash() {
 use super::{PendingCommit, classify_action};

 let c1 = PendingCommit {
 oid: "a".to_owned(),
 message: "one".to_owned(),
 author: "dev".to_owned(),
 timestamp: Utc::now(),
 changed_files: vec!["src/lib.rs".to_owned()],
 };
 let c2 = PendingCommit {
 oid: "b".to_owned(),
 message: "two".to_owned(),
 author: "dev".to_owned(),
 timestamp: Utc::now(),
 changed_files: vec!["src/lib.rs".to_owned()],
 };
 let group = vec![&c1, &c2];
 assert_eq!(classify_action(&group), ReplayAction::Squash);
 }

 #[test]
 fn group_into_sessions_splits_on_large_gap() {
 use super::{PendingCommit, group_into_sessions};
 use chrono::Duration;

 let t0 = Utc::now();
 let t1 = t0 + Duration::seconds(60); // same session (< 600 s gap)
 let t2 = t1 + Duration::seconds(700); // new session (> SESSION_GAP_SECONDS)
 let commits = vec![
 PendingCommit {
 oid: "a".to_owned(),
 message: "m".to_owned(),
 author: "x".to_owned(),
 timestamp: t0,
 changed_files: vec![],
 },
 PendingCommit {
 oid: "b".to_owned(),
 message: "m".to_owned(),
 author: "x".to_owned(),
 timestamp: t1,
 changed_files: vec![],
 },
 PendingCommit {
 oid: "c".to_owned(),
 message: "m".to_owned(),
 author: "x".to_owned(),
 timestamp: t2,
 changed_files: vec![],
 },
 ];
 let sessions = group_into_sessions(&commits);
 assert_eq!(sessions.len(), 2, "large gap should produce two sessions");
 assert_eq!(sessions[0].len(), 2);
 assert_eq!(sessions[1].len(), 1);
 }

 #[test]
 fn squash_groups_splits_on_non_overlapping_files() {
 use super::{PendingCommit, squash_groups};

 let c1 = PendingCommit {
 oid: "a".to_owned(),
 message: "m".to_owned(),
 author: "x".to_owned(),
 timestamp: Utc::now(),
 changed_files: vec!["src/a.rs".to_owned()],
 };
 let c2 = PendingCommit {
 oid: "b".to_owned(),
 message: "m".to_owned(),
 author: "x".to_owned(),
 timestamp: Utc::now(),
 changed_files: vec!["src/b.rs".to_owned()],
 };
 let session: Vec<&PendingCommit> = vec![&c1, &c2];
 let groups = squash_groups(&session);
 // No overlapping files → 2 separate groups
 assert_eq!(groups.len(), 2);
 }

 #[test]
 fn squash_groups_merges_overlapping_files() {
 use super::{PendingCommit, squash_groups};

 let c1 = PendingCommit {
 oid: "a".to_owned(),
 message: "m".to_owned(),
 author: "x".to_owned(),
 timestamp: Utc::now(),
 changed_files: vec!["src/lib.rs".to_owned()],
 };
 let c2 = PendingCommit {
 oid: "b".to_owned(),
 message: "m".to_owned(),
 author: "x".to_owned(),
 timestamp: Utc::now(),
 changed_files: vec!["src/lib.rs".to_owned()],
 };
 let session: Vec<&PendingCommit> = vec![&c1, &c2];
 let groups = squash_groups(&session);
 assert_eq!(groups.len(), 1);
 assert_eq!(groups[0].len(), 2);
 }

 #[test]
 fn next_active_time_empty_windows_returns_plus_one_hour() {
 // Covers line 366: windows.is_empty() → from + 1 hour.
 use super::next_active_time;
 use chrono::Utc;
 use chrono_tz::UTC;

 let from = Utc::now();
 let result = next_active_time(from, &[], UTC);
 let diff = result - from;
 assert!(
 diff.num_seconds() >= 3599 && diff.num_seconds() <= 3601,
 "empty windows should return from + 1 hour"
 );
 }
}