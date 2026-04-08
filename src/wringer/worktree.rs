use std::fs;
use std::path::{Path, PathBuf};

use git2::{BranchType, ObjectType, Repository, WorktreeAddOptions};

use crate::domain::errors::PapertowelError;

pub const COMPONENT_NAME: &str = "worktree";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeSpec {
    pub name: String,
    pub branch: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeStatus {
    pub name: String,
    pub branch: String,
    pub path: PathBuf,
    pub exists: bool,
}

pub fn initialize_worktree(
    repository_path: impl AsRef<Path>,
    spec: &WorktreeSpec,
) -> Result<WorktreeStatus, PapertowelError> {
    let repository = Repository::open(repository_path.as_ref())?;

    if worktree_exists(&repository, &spec.name)? {
        return status_worktree(repository_path, spec);
    }

    if let Some(parent) = spec.path.parent() {
        fs::create_dir_all(parent).map_err(|error| PapertowelError::io_with_path(parent, error))?;
    }

    let branch = ensure_branch_exists(&repository, &spec.branch)?;
    let branch_ref = branch.into_reference();

    let mut opts = WorktreeAddOptions::new();
    opts.reference(Some(&branch_ref));

    repository
        .worktree(&spec.name, &spec.path, Some(&opts))
        .map_err(PapertowelError::Git)?;

    status_worktree(repository_path, spec)
}

pub fn status_worktree(
    repository_path: impl AsRef<Path>,
    spec: &WorktreeSpec,
) -> Result<WorktreeStatus, PapertowelError> {
    let repository = Repository::open(repository_path.as_ref())?;
    let exists = worktree_exists(&repository, &spec.name)?;

    Ok(WorktreeStatus {
        name: spec.name.clone(),
        branch: spec.branch.clone(),
        path: spec.path.clone(),
        exists,
    })
}

pub fn remove_worktree(
    repository_path: impl AsRef<Path>,
    name: &str,
) -> Result<bool, PapertowelError> {
    let repository = Repository::open(repository_path.as_ref())?;
    let worktree = match repository.find_worktree(name) {
        Ok(worktree) => worktree,
        Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(false),
        Err(error) => return Err(PapertowelError::Git(error)),
    };

    let mut options = git2::WorktreePruneOptions::new();
    options.valid(true).working_tree(true).locked(true);
    worktree
        .prune(Some(&mut options))
        .map_err(PapertowelError::Git)?;
    Ok(true)
}

fn ensure_branch_exists<'repo>(
    repository: &'repo Repository,
    branch: &str,
) -> Result<git2::Branch<'repo>, PapertowelError> {
    if let Ok(existing) = repository.find_branch(branch, BranchType::Local) {
        return Ok(existing);
    }

    let head = repository.head().map_err(PapertowelError::Git)?;
    let target = head
        .peel(ObjectType::Commit)
        .map_err(PapertowelError::Git)?;
    let commit = target
        .into_commit()
        .map_err(|_| PapertowelError::Config("HEAD is not a commit".to_owned()))?;

    repository
        .branch(branch, &commit, false)
        .map_err(PapertowelError::Git)
}

fn worktree_exists(repository: &Repository, name: &str) -> Result<bool, PapertowelError> {
    let names = repository.worktrees().map_err(PapertowelError::Git)?;
    Ok(names
        .iter()
        .flatten()
        .any(|candidate| candidate.eq_ignore_ascii_case(name)))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use git2::{IndexAddOption, Repository, Signature};
    use tempfile::TempDir;

    use crate::wringer::worktree::{
        WorktreeSpec, initialize_worktree, remove_worktree, status_worktree,
    };

    #[test]
    fn initialize_worktree_creates_worktree_and_branch() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = TempDir::new()?;

        let repository_root = tmp.path().join("repo");
        fs::create_dir_all(&repository_root)?;

        let repository = init_repository_with_initial_commit(&repository_root)?;

        let worktree_path = tmp.path().join("public-worktree");
        let spec = WorktreeSpec {
            name: String::from("public"),
            branch: String::from("public"),
            path: worktree_path.clone(),
        };

        let status = initialize_worktree(&repository_root, &spec)?;
        assert!(status.exists);
        assert!(worktree_path.exists());
        assert!(
            repository
                .find_branch("public", git2::BranchType::Local)
                .is_ok()
        );
        Ok(())
    }

    #[test]
    fn remove_worktree_returns_false_when_missing() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = TempDir::new()?;

        let repository_root = tmp.path().join("repo");
        fs::create_dir_all(&repository_root)?;

        init_repository_with_initial_commit(&repository_root)?;

        let removed = remove_worktree(&repository_root, "missing")?;
        assert!(!removed);
        Ok(())
    }

    #[test]
    fn status_reports_nonexistent_worktree() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = TempDir::new()?;

        let repository_root = tmp.path().join("repo");
        fs::create_dir_all(&repository_root)?;

        init_repository_with_initial_commit(&repository_root)?;

        let spec = WorktreeSpec {
            name: String::from("public"),
            branch: String::from("public"),
            path: tmp.path().join("public-worktree"),
        };

        let status = status_worktree(&repository_root, &spec)?;
        assert!(!status.exists);
        Ok(())
    }

    fn init_repository_with_initial_commit(path: &Path) -> Result<Repository, git2::Error> {
        let repository = Repository::init(path)?;
        fs::write(path.join("README.md"), "# test\n")
            .map_err(|error| git2::Error::from_str(&error.to_string()))?;

        let mut index = repository.index()?;
        index.add_all(std::iter::once(&"*"), IndexAddOption::DEFAULT, None)?;
        index.write()?;

        let tree_oid = index.write_tree()?;
        {
            let tree = repository.find_tree(tree_oid)?;
            let signature = Signature::now("papertowel", "papertowel@example.com")?;
            repository.commit(
                Some("HEAD"),
                &signature,
                &signature,
                "initial commit",
                &tree,
                &[],
            )?;
        }

        Ok(repository)
    }
}
