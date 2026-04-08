use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use git2::{Repository, Signature};
use rand::SeedableRng;
use rand::rngs::StdRng;

use crate::{
    domain::errors::PapertowelError,
    profile::persona::PersonaArchaeology,
    wringer::{
        archaeology::inject_before_entry,
        config::{load_wringer_config, WringerConfig},
        queue::{load_queue_plan, save_queue_plan, QueueEntry},
    },
};

pub const COMPONENT_NAME: &str = "drip";

/// Stats from a single `tick()` run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DripStats {
    /// Entries applied in this tick.
    pub applied: usize,
    /// Entries that are scheduled in the future and were skipped.
    pub pending: usize,
    /// Entries that were already completed and were skipped.
    pub already_done: usize,
}

/// Daemon that replays pending queue entries into the public worktree on
/// schedule.  Call [`DripRunner::tick`] periodically (or once for a
/// single-shot run).
pub struct DripRunner {
    repo_root: PathBuf,
    config: WringerConfig,
    /// Optional archaeology settings; when present, synthetic commits are
    /// injected into the worktree before each real entry is applied.
    archaeology: Option<PersonaArchaeology>,
    rng: StdRng,
}

impl DripRunner {
    /// Build a new `DripRunner` by loading the persisted wringer config for
    /// `repo_root`.
    pub fn new(repo_root: impl AsRef<Path>) -> Result<Self, PapertowelError> {
        let root = repo_root.as_ref().to_owned();
        let config = load_wringer_config(&root)?;
        Ok(Self {
            repo_root: root,
            config,
            archaeology: None,
            rng: StdRng::from_entropy(),
        })
    }

    /// Enable archaeology injection with the given persona settings.
    #[must_use]
    pub fn with_archaeology(mut self, settings: PersonaArchaeology) -> Self {
        self.archaeology = Some(settings);
        self
    }

    /// Override the RNG seed for deterministic testing.
    #[must_use]
    pub fn with_rng_seed(mut self, seed: u64) -> Self {
        self.rng = StdRng::seed_from_u64(seed);
        self
    }

    /// Apply all queue entries whose `target_time` is in the past and that
    /// are not yet completed.  Marks applied entries as completed and
    /// persists the updated plan.
    ///
    /// Returns statistics for the current tick.
    pub fn tick(&mut self) -> Result<DripStats, PapertowelError> {
        self.tick_at(Utc::now())
    }

    /// Like [`tick`], but uses `now` as the reference timestamp.  Useful for
    /// deterministic testing.
    pub fn tick_at(&mut self, now: DateTime<Utc>) -> Result<DripStats, PapertowelError> {
        let mut plan = load_queue_plan(&self.repo_root)?;

        let mut applied: usize = 0;
        let mut pending: usize = 0;
        let mut already_done: usize = 0;

        for entry in &mut plan.entries {
            if entry.completed {
                already_done += 1;
                continue;
            }

            if entry.target_time > now {
                pending += 1;
                continue;
            }

            // Optionally inject synthetic archaeology commits before the real
            // cherry-pick so the public history looks more organic.
            if let Some(ref settings) = self.archaeology.clone() {
                let _ = inject_before_entry(
                    &self.config.worktree_path,
                    entry,
                    settings,
                    &mut self.rng,
                );
            }

            apply_entry(&self.config.worktree_path, entry)?;
            entry.completed = true;
            applied += 1;
        }

        if applied > 0 {
            save_queue_plan(&self.repo_root, &plan)?;
        }

        Ok(DripStats {
            applied,
            pending,
            already_done,
        })
    }

    /// Return the number of entries that are pending (not yet completed and
    /// scheduled in the future).
    pub fn pending_count(&self) -> Result<usize, PapertowelError> {
        let plan = load_queue_plan(&self.repo_root)?;
        let now = Utc::now();
        Ok(plan
            .entries
            .iter()
            .filter(|e| !e.completed && e.target_time > now)
            .count())
    }
}

// ─── Internal helpers ────────────────────────────────────────────────────────

/// Cherry-pick all source commits from a queue entry onto the worktree HEAD,
/// then create a single commit with `entry.message` and `entry.target_time`
/// as the commit timestamp.
fn apply_entry(worktree_path: &Path, entry: &QueueEntry) -> Result<(), PapertowelError> {
    let wt_repo = Repository::open(worktree_path)?;

    // Resolve author/committer identity from the repo config; fall back to a
    // neutral papertowel identity when not configured.
    let (author_name, author_email) = resolve_identity(&wt_repo);

    // Use entry.target_time as the commit timestamp so the public history
    // reflects the persona's realistic schedule.
    let sig_time = git2::Time::new(entry.target_time.timestamp(), 0);
    let sig = Signature::new(&author_name, &author_email, &sig_time)?;

    let mut pick_opts = git2::CherrypickOptions::new();

    for oid_str in &entry.source_oids {
        let oid: git2::Oid = oid_str
            .parse()
            .map_err(|e| PapertowelError::Config(format!("invalid OID {oid_str:?}: {e}")))?;

        let commit = wt_repo.find_commit(oid)?;

        // Merge commits require a mainline declaration (1-indexed parent).
        // Regular commits do not need it; setting mainline to 0 is a no-op
        // in git2 (it means "not a merge commit").
        if commit.parent_count() >= 2 {
            pick_opts.mainline(1);
        } else {
            pick_opts.mainline(0);
        }

        wt_repo.cherrypick(&commit, Some(&mut pick_opts))?;
    }

    // Validate there are no conflicts before committing.
    let mut index = wt_repo.index()?;
    index.read(false)?;

    if index.has_conflicts() {
        let _ = wt_repo.cleanup_state();
        return Err(PapertowelError::Config(format!(
            "cherry-pick conflict; source OIDs: {}",
            entry.source_oids.join(", ")
        )));
    }

    let tree_oid = index.write_tree()?;
    let tree = wt_repo.find_tree(tree_oid)?;

    let head = wt_repo.head()?;
    let parent = head.peel_to_commit()?;

    wt_repo.commit(Some("HEAD"), &sig, &sig, &entry.message, &tree, &[&parent])?;

    wt_repo.cleanup_state()?;

    Ok(())
}

/// Retrieve the user name and email from the repository git config, falling
/// back to neutral defaults when they are absent.
fn resolve_identity(repo: &Repository) -> (String, String) {
    let config = repo.config().ok();
    let name = config
        .as_ref()
        .and_then(|c| c.get_string("user.name").ok())
        .unwrap_or_else(|| String::from("papertowel-wringer"));
    let email = config
        .as_ref()
        .and_then(|c| c.get_string("user.email").ok())
        .unwrap_or_else(|| String::from("wringer@papertowel.local"));
    (name, email)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::error::Error;

    use chrono::{Duration, Utc};
    use tempfile::TempDir;

    use crate::wringer::{
        config::{save_wringer_config, WringerConfig},
        queue::{save_queue_plan, QueueEntry, QueuePlan, ReplayAction},
    };

    use super::DripRunner;

    fn minimal_config(tmp: &TempDir) -> WringerConfig {
        WringerConfig {
            branch: String::from("public"),
            worktree_name: String::from("public"),
            worktree_path: tmp.path().join("worktree"),
        }
    }

    fn no_entry_plan() -> QueuePlan {
        QueuePlan {
            sync_point: None,
            persona_name: String::from("test"),
            entries: Vec::new(),
            generated_at: Utc::now(),
        }
    }

    #[test]
    fn tick_returns_zero_stats_when_no_entries() -> Result<(), Box<dyn Error>> {
        let tmp = TempDir::new()?;
        save_wringer_config(tmp.path(), &minimal_config(&tmp))?;
        save_queue_plan(tmp.path(), &no_entry_plan())?;

        let mut runner = DripRunner::new(tmp.path())?;
        let stats = runner.tick_at(Utc::now())?;

        assert_eq!(stats.applied, 0);
        assert_eq!(stats.pending, 0);
        assert_eq!(stats.already_done, 0);
        Ok(())
    }

    #[test]
    fn tick_skips_future_entries() -> Result<(), Box<dyn Error>> {
        let tmp = TempDir::new()?;
        save_wringer_config(tmp.path(), &minimal_config(&tmp))?;

        let plan = QueuePlan {
            sync_point: None,
            persona_name: String::from("test"),
            entries: vec![QueueEntry {
                source_oids: vec![String::from("deadbeef")],
                message: String::from("future work"),
                target_time: Utc::now() + Duration::hours(3),
                action: ReplayAction::Replay,
                completed: false,
            }],
            generated_at: Utc::now(),
        };
        save_queue_plan(tmp.path(), &plan)?;

        let mut runner = DripRunner::new(tmp.path())?;
        let stats = runner.tick_at(Utc::now())?;

        assert_eq!(stats.applied, 0, "future entries must not be applied");
        assert_eq!(stats.pending, 1);
        Ok(())
    }

    #[test]
    fn tick_skips_already_completed_entries() -> Result<(), Box<dyn Error>> {
        let tmp = TempDir::new()?;
        save_wringer_config(tmp.path(), &minimal_config(&tmp))?;

        let past = Utc::now() - Duration::hours(1);
        let plan = QueuePlan {
            sync_point: None,
            persona_name: String::from("test"),
            entries: vec![QueueEntry {
                source_oids: vec![String::from("deadbeef")],
                message: String::from("already done"),
                target_time: past,
                action: ReplayAction::Replay,
                completed: true,
            }],
            generated_at: Utc::now(),
        };
        save_queue_plan(tmp.path(), &plan)?;

        let mut runner = DripRunner::new(tmp.path())?;
        let stats = runner.tick_at(Utc::now())?;

        assert_eq!(stats.applied, 0);
        assert_eq!(stats.already_done, 1);
        Ok(())
    }

    #[test]
    fn pending_count_counts_only_future_uncompleted() -> Result<(), Box<dyn Error>> {
        let tmp = TempDir::new()?;
        save_wringer_config(tmp.path(), &minimal_config(&tmp))?;

        let plan = QueuePlan {
            sync_point: None,
            persona_name: String::from("test"),
            entries: vec![
                QueueEntry {
                    source_oids: vec![String::from("aaa")],
                    message: String::from("completed past"),
                    target_time: Utc::now() - Duration::hours(2),
                    action: ReplayAction::Replay,
                    completed: true,
                },
                QueueEntry {
                    source_oids: vec![String::from("bbb")],
                    message: String::from("pending future"),
                    target_time: Utc::now() + Duration::hours(2),
                    action: ReplayAction::Replay,
                    completed: false,
                },
                QueueEntry {
                    source_oids: vec![String::from("ccc")],
                    message: String::from("also pending future"),
                    target_time: Utc::now() + Duration::hours(4),
                    action: ReplayAction::Replay,
                    completed: false,
                },
            ],
            generated_at: Utc::now(),
        };
        save_queue_plan(tmp.path(), &plan)?;

        let runner = DripRunner::new(tmp.path())?;
        assert_eq!(runner.pending_count()?, 2, "2 future uncompleted entries");
        Ok(())
    }
}