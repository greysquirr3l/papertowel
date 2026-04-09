use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Args;

const PRE_COMMIT_HOOK: &str = r#"#!/bin/sh
# papertowel pre-commit hook — scans staged files for AI fingerprints.
# Installed by: papertowel hook install
# Remove with:  papertowel hook uninstall

set -e

# Collect staged files (excluding deleted).
STAGED=$(git diff --cached --name-only --diff-filter=d)
if [ -z "$STAGED" ]; then
    exit 0
fi

# Build a temp dir with only the staged versions of files so we scan
# exactly what is being committed, not the working-tree copy.
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

for f in $STAGED; do
    mkdir -p "$TMPDIR/$(dirname "$f")"
    git show ":$f" > "$TMPDIR/$f" 2>/dev/null || true
done

exec papertowel scan "$TMPDIR" --fail-on medium
"#;

#[derive(Debug, Args)]
pub struct InstallArgs {
    /// Overwrite an existing pre-commit hook without prompting.
    #[arg(long, default_value_t = false)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct UninstallArgs;

#[derive(Debug, Args)]
pub struct StatusArgs;

/// Resolve the hooks directory for the current repository.
fn hooks_dir() -> Result<PathBuf> {
    let repo = git2::Repository::discover(".").context("not inside a git repository")?;
    let git_dir = repo.path().to_path_buf(); // .git/
    Ok(git_dir.join("hooks"))
}

fn pre_commit_path() -> Result<PathBuf> {
    Ok(hooks_dir()?.join("pre-commit"))
}

fn is_papertowel_hook(path: &Path) -> bool {
    fs::read_to_string(path)
        .map(|content| content.contains("papertowel"))
        .unwrap_or(false)
}

pub fn handle_install(args: &InstallArgs) -> Result<()> {
    let hook_path = pre_commit_path()?;
    let hooks = hook_path
        .parent()
        .context("cannot resolve hooks directory")?;

    if hook_path.exists() && !args.force {
        if is_papertowel_hook(&hook_path) {
            println!("papertowel pre-commit hook is already installed.");
            return Ok(());
        }
        anyhow::bail!(
            "a pre-commit hook already exists at {}\nuse --force to overwrite",
            hook_path.display()
        );
    }

    fs::create_dir_all(hooks)
        .with_context(|| format!("failed to create hooks dir {}", hooks.display()))?;

    fs::write(&hook_path, PRE_COMMIT_HOOK)
        .with_context(|| format!("failed to write hook at {}", hook_path.display()))?;

    let mut perms = fs::metadata(&hook_path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&hook_path, perms)?;

    println!(
        "Installed papertowel pre-commit hook at {}",
        hook_path.display()
    );
    Ok(())
}

pub fn handle_uninstall(_args: &UninstallArgs) -> Result<()> {
    let hook_path = pre_commit_path()?;

    if !hook_path.exists() {
        println!("No pre-commit hook found.");
        return Ok(());
    }

    if !is_papertowel_hook(&hook_path) {
        anyhow::bail!(
            "pre-commit hook at {} was not installed by papertowel — refusing to remove",
            hook_path.display()
        );
    }

    fs::remove_file(&hook_path)
        .with_context(|| format!("failed to remove {}", hook_path.display()))?;

    println!("Removed papertowel pre-commit hook.");
    Ok(())
}

pub fn handle_status(_args: &StatusArgs) -> Result<()> {
    let hook_path = pre_commit_path()?;

    if !hook_path.exists() {
        println!("No pre-commit hook installed.");
        return Ok(());
    }

    if is_papertowel_hook(&hook_path) {
        println!(
            "papertowel pre-commit hook is installed at {}",
            hook_path.display()
        );
    } else {
        println!(
            "A pre-commit hook exists at {} but was not installed by papertowel.",
            hook_path.display()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #![expect(clippy::expect_used, reason = "test assertions")]
    #![expect(clippy::unwrap_used, reason = "test assertions")]

    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use tempfile::TempDir;

    use super::*;

    /// Tests that change cwd must hold this lock to avoid racing each other.
    static CWD_LOCK: Mutex<()> = Mutex::new(());

    fn setup_repo() -> (TempDir, PathBuf) {
        let dir = TempDir::new().expect("tempdir");
        git2::Repository::init(dir.path()).expect("git init");
        let hooks = dir.path().join(".git").join("hooks");
        fs::create_dir_all(&hooks).expect("create hooks dir");
        (dir, hooks)
    }

    #[test]
    fn install_creates_executable_hook() {
        let _lock = CWD_LOCK.lock();
        let (dir, hooks) = setup_repo();
        let _guard = SetCwd::new(dir.path());

        let args = InstallArgs { force: false };
        handle_install(&args).expect("install should succeed");

        let hook = hooks.join("pre-commit");
        assert!(hook.exists());

        let content = fs::read_to_string(&hook).expect("read hook");
        assert!(content.contains("papertowel"));
        assert!(content.contains("papertowel scan"));

        let mode = fs::metadata(&hook).expect("metadata").permissions().mode();
        assert_ne!(mode & 0o111, 0, "hook must be executable");
    }

    #[test]
    fn install_idempotent_when_already_installed() {
        let _lock = CWD_LOCK.lock();
        let (dir, _hooks) = setup_repo();
        let _guard = SetCwd::new(dir.path());

        let args = InstallArgs { force: false };
        handle_install(&args).expect("first install");
        handle_install(&args).expect("second install should be idempotent");
    }

    #[test]
    fn install_refuses_to_overwrite_foreign_hook() {
        let _lock = CWD_LOCK.lock();
        let (dir, hooks) = setup_repo();
        let _guard = SetCwd::new(dir.path());

        let hook = hooks.join("pre-commit");
        fs::write(&hook, "#!/bin/sh\necho foreign hook\n").expect("write foreign");

        let args = InstallArgs { force: false };
        let result = handle_install(&args);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("use --force to overwrite")
        );
    }

    #[test]
    fn install_force_overwrites_foreign_hook() {
        let _lock = CWD_LOCK.lock();
        let (dir, hooks) = setup_repo();
        let _guard = SetCwd::new(dir.path());

        let hook = hooks.join("pre-commit");
        fs::write(&hook, "#!/bin/sh\necho foreign hook\n").expect("write foreign");

        let args = InstallArgs { force: true };
        handle_install(&args).expect("force install should succeed");

        let content = fs::read_to_string(&hook).expect("read hook");
        assert!(content.contains("papertowel"));
    }

    #[test]
    fn uninstall_removes_papertowel_hook() {
        let _lock = CWD_LOCK.lock();
        let (dir, hooks) = setup_repo();
        let _guard = SetCwd::new(dir.path());

        let hook = hooks.join("pre-commit");
        fs::write(&hook, PRE_COMMIT_HOOK).expect("write hook");

        handle_uninstall(&UninstallArgs).expect("uninstall");
        assert!(!hook.exists());
    }

    #[test]
    fn uninstall_refuses_to_remove_foreign_hook() {
        let _lock = CWD_LOCK.lock();
        let (dir, hooks) = setup_repo();
        let _guard = SetCwd::new(dir.path());

        let hook = hooks.join("pre-commit");
        fs::write(&hook, "#!/bin/sh\necho foreign\n").expect("write foreign");

        let result = handle_uninstall(&UninstallArgs);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("not installed by papertowel")
        );
    }

    #[test]
    fn uninstall_noop_when_no_hook() {
        let _lock = CWD_LOCK.lock();
        let (dir, _hooks) = setup_repo();
        let _guard = SetCwd::new(dir.path());

        handle_uninstall(&UninstallArgs).expect("uninstall noop");
    }

    /// RAII guard that sets the working directory for test isolation.
    struct SetCwd {
        prev: PathBuf,
    }

    impl SetCwd {
        fn new(path: &Path) -> Self {
            let prev = std::env::current_dir().expect("cwd");
            std::env::set_current_dir(path).expect("chdir");
            Self { prev }
        }
    }

    impl Drop for SetCwd {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.prev);
        }
    }
}
