use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use git2::{Repository, Signature};
use rand::Rng;
use walkdir::WalkDir;

use crate::domain::errors::PapertowelError;
use crate::profile::persona::PersonaArchaeology;
use crate::wringer::queue::QueueEntry;

pub const COMPONENT_NAME: &str = "archaeology";

/// TODO comment pool: plausible short annotations a developer might add while
/// coding and later clean up.
static TODO_COMMENTS: &[&str] = &[
    "// TODO: review error handling here",
    "// TODO: add tests for edge cases",
    "// TODO: clean up before shipping",
    "// TODO: refactor this section",
    "// TODO: double-check the logic",
    "// FIXME: might need to revisit this",
    "// TODO: perf — consider caching",
    "// TODO: document this",
];

/// Dead-code snippets: commented-out lines that look like half-finished ideas.
static DEAD_CODE_SNIPPETS: &[&str] = &[
    "\n// let _result = todo!();\n",
    "\n// eprintln!(\"debug: reached this branch\");\n",
    "\n// let _tmp = Vec::<u8>::new();\n",
    "\n// assert!(false, \"unreachable?\");\n",
];

/// real commit represented by `entry` is applied.
///
///
pub fn inject_before_entry(
    worktree_path: &Path,
    entry: &QueueEntry,
    settings: &PersonaArchaeology,
    rng: &mut impl Rng,
) -> Result<usize, PapertowelError> {
    let mut count: usize = 0;

    if rng.random_range(0.0_f32..1.0_f32) < settings.todo_inject_rate {
        count += inject_todo_pair(worktree_path, entry, rng)?;
    }

    if rng.random_range(0.0_f32..1.0_f32) < settings.dead_code_rate {
        count += inject_dead_code_pair(worktree_path, entry, rng)?;
    }

    Ok(count)
}

// ─── Injection backends ──────────────────────────────────────────────────────

/// Add a TODO comment to a random `.rs` file, then remove it in a follow-up
/// commit. Net effect on the working tree: zero.
fn inject_todo_pair(
    worktree_path: &Path,
    entry: &QueueEntry,
    rng: &mut impl Rng,
) -> Result<usize, PapertowelError> {
    let rs_files = collect_rs_files(worktree_path);
    if rs_files.is_empty() {
        return Ok(0);
    }

    let Some(target) = rs_files.get(rng.random_range(0..rs_files.len())) else {
        return Ok(0); // unreachable given the empty check above
    };
    let rel = relative_to(target, worktree_path)?;

    let original =
        fs::read_to_string(target).map_err(|e| PapertowelError::io_with_path(target, e))?;

    let Some(&comment) = TODO_COMMENTS.get(rng.random_range(0..TODO_COMMENTS.len())) else {
        return Ok(0);
    };
    let with_todo = format!("{original}\n{comment}\n");

    let repo = Repository::open(worktree_path)?;

    let add_sig = build_sig(&repo, entry.target_time - Duration::minutes(30))?;
    fs::write(target, &with_todo).map_err(|e| PapertowelError::io_with_path(target, e))?;
    stage_and_commit(&repo, &rel, &add_sig, "wip: review todos")?;

    let rm_sig = build_sig(&repo, entry.target_time - Duration::minutes(10))?;
    fs::write(target, &original).map_err(|e| PapertowelError::io_with_path(target, e))?;
    stage_and_commit(&repo, &rel, &rm_sig, "cleanup: remove stale todo")?;

    Ok(2)
}

/// Inject a commented-out dead-code snippet, then remove it in a follow-up
/// commit. Net effect on the working tree: zero.
fn inject_dead_code_pair(
    worktree_path: &Path,
    entry: &QueueEntry,
    rng: &mut impl Rng,
) -> Result<usize, PapertowelError> {
    let rs_files = collect_rs_files(worktree_path);
    if rs_files.is_empty() {
        return Ok(0);
    }

    let Some(target) = rs_files.get(rng.random_range(0..rs_files.len())) else {
        return Ok(0);
    };
    let rel = relative_to(target, worktree_path)?;

    let original =
        fs::read_to_string(target).map_err(|e| PapertowelError::io_with_path(target, e))?;

    let Some(&snippet) = DEAD_CODE_SNIPPETS.get(rng.random_range(0..DEAD_CODE_SNIPPETS.len()))
    else {
        return Ok(0);
    };
    let with_dead = format!("{original}{snippet}");

    let repo = Repository::open(worktree_path)?;

    let add_sig = build_sig(&repo, entry.target_time - Duration::minutes(45))?;
    fs::write(target, &with_dead).map_err(|e| PapertowelError::io_with_path(target, e))?;
    stage_and_commit(&repo, &rel, &add_sig, "temp: scratch work")?;

    let rm_sig = build_sig(&repo, entry.target_time - Duration::minutes(20))?;
    fs::write(target, &original).map_err(|e| PapertowelError::io_with_path(target, e))?;
    stage_and_commit(&repo, &rel, &rm_sig, "cleanup: remove scratch")?;

    Ok(2)
}

// ─── Internal helpers ────────────────────────────────────────────────────────

/// Collect all `.rs` files under `root`, skipping `.git` and `target`.
fn collect_rs_files(root: &Path) -> Vec<PathBuf> {
    WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| {
            if !e.file_type().is_file() {
                return false;
            }
            let Some(ext) = e.path().extension() else {
                return false;
            };
            if ext != "rs" {
                return false;
            }
            !e.path().components().any(|c| {
                let s = c.as_os_str();
                s == ".git" || s == "target"
            })
        })
        .map(walkdir::DirEntry::into_path)
        .collect()
}

/// file does not live under `root`.
fn relative_to(target: &Path, root: &Path) -> Result<PathBuf, PapertowelError> {
    target
        .strip_prefix(root)
        .map(ToOwned::to_owned)
        .map_err(|_| {
            PapertowelError::Config(format!(
                "file {} is outside worktree root {}",
                target.display(),
                root.display()
            ))
        })
}

/// Build a git2 `Signature` stamped with the given timestamp, pulling name
fn build_sig(repo: &Repository, at: DateTime<Utc>) -> Result<Signature<'static>, PapertowelError> {
    let config = repo.config().ok();
    let name = config
        .as_ref()
        .and_then(|c| c.get_string("user.name").ok())
        .unwrap_or_else(|| String::from("papertowel-wringer"));
    let email = config
        .as_ref()
        .and_then(|c| c.get_string("user.email").ok())
        .unwrap_or_else(|| String::from("wringer@papertowel.local"));
    let time = git2::Time::new(at.timestamp(), 0);
    Signature::new(&name, &email, &time).map_err(PapertowelError::Git)
}

/// Stage `rel_path` and create a commit on HEAD with the given message.
fn stage_and_commit(
    repo: &Repository,
    rel_path: &Path,
    sig: &Signature<'_>,
    message: &str,
) -> Result<git2::Oid, PapertowelError> {
    let mut index = repo.index()?;
    index.add_path(rel_path)?;
    index.write()?;

    let tree_oid = index.write_tree()?;
    let tree = repo.find_tree(tree_oid)?;

    let head = repo.head()?;
    let parent = head.peel_to_commit()?;

    let oid = repo.commit(Some("HEAD"), sig, sig, message, &tree, &[&parent])?;
    Ok(oid)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::fs;
    use std::path::Path;

    use chrono::Utc;
    use git2::{IndexAddOption, Repository, Signature};
    use rand::SeedableRng;
    use rand::rngs::StdRng;
    use tempfile::TempDir;

    use crate::profile::persona::PersonaArchaeology;
    use crate::wringer::queue::{QueueEntry, ReplayAction};

    use super::{collect_rs_files, inject_before_entry};

    fn init_repo_with_rs_file(path: &Path) -> Result<Repository, git2::Error> {
        let repo = Repository::init(path)?;

        let src = path.join("src");
        fs::create_dir_all(&src).map_err(|e| git2::Error::from_str(&e.to_string()))?;
        fs::write(src.join("lib.rs"), "pub fn hello() {}\n")
            .map_err(|e| git2::Error::from_str(&e.to_string()))?;

        let mut index = repo.index()?;
        index.add_all(std::iter::once(&"*"), IndexAddOption::DEFAULT, None)?;
        index.write()?;

        let tree_oid = index.write_tree()?;
        {
            let tree = repo.find_tree(tree_oid)?;
            let sig = Signature::now("test", "test@example.com")?;
            repo.commit(Some("HEAD"), &sig, &sig, "initial commit", &tree, &[])?;
        }

        Ok(repo)
    }

    fn count_head_commits(repo_path: &Path) -> Result<usize, Box<dyn Error>> {
        let repo = Repository::open(repo_path)?;
        let mut walker = repo.revwalk()?;
        walker.push_head()?;
        Ok(walker.filter_map(Result::ok).count())
    }

    fn dummy_entry() -> QueueEntry {
        QueueEntry {
            source_oids: vec![String::from("deadbeef")],
            message: String::from("wip: some work"),
            target_time: Utc::now(),
            action: ReplayAction::Replay,
            completed: false,
        }
    }

    #[test]
    fn zero_rates_injects_nothing() -> Result<(), Box<dyn Error>> {
        let tmp = TempDir::new()?;
        init_repo_with_rs_file(tmp.path())?;

        let settings = PersonaArchaeology {
            todo_inject_rate: 0.0,
            dead_code_rate: 0.0,
            rename_chains: false,
        };
        let mut rng = StdRng::seed_from_u64(42);

        let count = inject_before_entry(tmp.path(), &dummy_entry(), &settings, &mut rng)?;
        assert_eq!(count, 0);
        assert_eq!(count_head_commits(tmp.path())?, 1, "no new commits");
        Ok(())
    }

    #[test]
    fn todo_rate_one_creates_two_commits() -> Result<(), Box<dyn Error>> {
        let tmp = TempDir::new()?;
        init_repo_with_rs_file(tmp.path())?;

        let settings = PersonaArchaeology {
            todo_inject_rate: 1.0,
            dead_code_rate: 0.0,
            rename_chains: false,
        };
        let mut rng = StdRng::seed_from_u64(7);

        let count = inject_before_entry(tmp.path(), &dummy_entry(), &settings, &mut rng)?;
        assert_eq!(count, 2, "one TODO pair = 2 commits");
        assert_eq!(
            count_head_commits(tmp.path())?,
            3,
            "initial + add_todo + rm_todo"
        );
        Ok(())
    }

    #[test]
    fn dead_code_rate_one_creates_two_commits() -> Result<(), Box<dyn Error>> {
        let tmp = TempDir::new()?;
        init_repo_with_rs_file(tmp.path())?;

        let settings = PersonaArchaeology {
            todo_inject_rate: 0.0,
            dead_code_rate: 1.0,
            rename_chains: false,
        };
        let mut rng = StdRng::seed_from_u64(13);

        let count = inject_before_entry(tmp.path(), &dummy_entry(), &settings, &mut rng)?;
        assert_eq!(count, 2, "one dead-code pair = 2 commits");
        assert_eq!(count_head_commits(tmp.path())?, 3);
        Ok(())
    }

    #[test]
    fn both_rates_one_creates_four_commits() -> Result<(), Box<dyn Error>> {
        let tmp = TempDir::new()?;
        init_repo_with_rs_file(tmp.path())?;

        let settings = PersonaArchaeology {
            todo_inject_rate: 1.0,
            dead_code_rate: 1.0,
            rename_chains: false,
        };
        let mut rng = StdRng::seed_from_u64(99);

        let count = inject_before_entry(tmp.path(), &dummy_entry(), &settings, &mut rng)?;
        assert_eq!(count, 4, "both pairs = 4 commits");
        assert_eq!(count_head_commits(tmp.path())?, 5);
        Ok(())
    }

    #[test]
    fn collect_rs_files_skips_git_and_target() -> Result<(), Box<dyn Error>> {
        let tmp = TempDir::new()?;
        let root = tmp.path();

        fs::create_dir_all(root.join("src"))?;
        fs::write(root.join("src/lib.rs"), "")?;
        fs::create_dir_all(root.join("target/debug"))?;
        fs::write(root.join("target/debug/output.rs"), "")?;
        fs::create_dir_all(root.join(".git"))?;
        fs::write(root.join(".git/FETCH_HEAD"), "")?;

        let files = collect_rs_files(root);
        assert_eq!(files.len(), 1);
        assert!(files.first().is_some_and(|f| f.ends_with("src/lib.rs")));
        Ok(())
    }

    #[test]
    fn relative_to_returns_error_when_outside_root() {
        use super::relative_to;
        use std::path::Path;
        let root = Path::new("/tmp/worktree");
        let outside = Path::new("/home/user/other/file.rs");
        let result = relative_to(outside, root);
        assert!(
            result.is_err(),
            "file outside worktree root should produce an error"
        );
    }

    #[test]
    fn inject_before_entry_returns_zero_for_empty_dir() -> Result<(), Box<dyn Error>> {
        use crate::profile::persona::PersonaArchaeology;
        use rand::SeedableRng;
        use rand::rngs::StdRng;
        let tmp = TempDir::new()?;
        let settings = PersonaArchaeology {
            todo_inject_rate: 1.0,
            dead_code_rate: 1.0,
            rename_chains: false,
        };
        let mut rng = StdRng::seed_from_u64(7);
        let count = inject_before_entry(tmp.path(), &dummy_entry(), &settings, &mut rng)?;
        assert_eq!(count, 0);
        Ok(())
    }
}
