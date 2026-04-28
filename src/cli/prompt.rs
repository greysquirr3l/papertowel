use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Args;

/// Universal agent instructions (works with any AI coding assistant).
const AGENTS_MD: &str = include_str!("../templates/agents.md");
/// God-file decomposition workflow — plain markdown, no editor-specific frontmatter.
const GOD_IS_DEAD_MD: &str = include_str!("../templates/god-is-dead.md");
/// VS Code–specific prompt format with tool/agent frontmatter.
const GOD_IS_DEAD_PROMPT_MD: &str = include_str!("../templates/god-is-dead.prompt.md");

#[derive(Debug, Args)]
pub struct InstallArgs {
    /// Project root to install prompt files into.
    #[arg(default_value = ".")]
    pub path: String,
    /// Overwrite existing files if present.
    #[arg(long, default_value_t = false)]
    pub force: bool,
    /// Also install the VS Code `.prompt.md` variant (requires VS Code + Copilot agent mode).
    #[arg(long, default_value_t = false)]
    pub vscode: bool,
}

#[derive(Debug, Args)]
pub struct ListArgs {}

pub fn handle_install(args: &InstallArgs) -> Result<()> {
    let root = PathBuf::from(&args.path);

    // Universal files: work with any AI coding assistant.
    let agents_path = root.join("AGENTS.md");
    let workflow_path = root.join(".papertowel").join("god-is-dead.md");

    write_with_force(&agents_path, AGENTS_MD, args.force)?;
    write_with_force(&workflow_path, GOD_IS_DEAD_MD, args.force)?;

    println!("Installed {}", agents_path.display());
    println!("Installed {}", workflow_path.display());

    if args.vscode {
        let vscode_prompt = root.join(".vscode").join("god-is-dead.prompt.md");
        write_with_force(&vscode_prompt, GOD_IS_DEAD_PROMPT_MD, args.force)?;
        println!("Installed {}", vscode_prompt.display());
    }

    Ok(())
}

pub fn handle_list(_args: &ListArgs) {
    println!("Available prompt templates:");
    println!(
        "  AGENTS.md                         Universal AI agent instructions (installed by default)"
    );
    println!(
        "  .papertowel/god-is-dead.md        God-file refactor workflow (installed by default)"
    );
    println!("  .vscode/god-is-dead.prompt.md     VS Code Copilot agent prompt (--vscode flag)");
}

fn write_with_force(path: &Path, content: &str, force: bool) -> Result<()> {
    if path.exists() && !force {
        anyhow::bail!(
            "{} already exists (use --force to overwrite)",
            path.display()
        );
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![expect(clippy::expect_used, reason = "test assertions")]

    use std::fs;

    use tempfile::TempDir;

    use super::{InstallArgs, ListArgs, handle_install, handle_list};

    #[test]
    fn install_writes_universal_files() {
        let dir = TempDir::new().expect("tempdir");

        handle_install(&InstallArgs {
            path: dir.path().to_string_lossy().into_owned(),
            force: false,
            vscode: false,
        })
        .expect("install should succeed");

        assert!(dir.path().join("AGENTS.md").exists());
        assert!(
            dir.path()
                .join(".papertowel")
                .join("god-is-dead.md")
                .exists()
        );
        assert!(
            !dir.path()
                .join(".vscode")
                .join("god-is-dead.prompt.md")
                .exists()
        );
    }

    #[test]
    fn install_with_vscode_flag_adds_prompt_file() {
        let dir = TempDir::new().expect("tempdir");

        handle_install(&InstallArgs {
            path: dir.path().to_string_lossy().into_owned(),
            force: false,
            vscode: true,
        })
        .expect("install should succeed");

        assert!(
            dir.path()
                .join(".vscode")
                .join("god-is-dead.prompt.md")
                .exists()
        );
    }

    #[test]
    fn install_refuses_to_overwrite_without_force() {
        let dir = TempDir::new().expect("tempdir");
        let agents = dir.path().join("AGENTS.md");
        fs::write(&agents, "existing").expect("write existing AGENTS.md");

        let result = handle_install(&InstallArgs {
            path: dir.path().to_string_lossy().into_owned(),
            force: false,
            vscode: false,
        });

        assert!(result.is_err());
    }

    #[test]
    fn install_force_overwrites_existing_files() {
        let dir = TempDir::new().expect("tempdir");

        handle_install(&InstallArgs {
            path: dir.path().to_string_lossy().into_owned(),
            force: false,
            vscode: false,
        })
        .expect("first install should succeed");

        let agents = dir.path().join("AGENTS.md");
        fs::write(&agents, "old content").expect("overwrite with old content");

        handle_install(&InstallArgs {
            path: dir.path().to_string_lossy().into_owned(),
            force: true,
            vscode: false,
        })
        .expect("force install should succeed");

        let after = fs::read_to_string(&agents).expect("read rewritten AGENTS.md");
        assert!(after.contains("AI Coding Agent Instructions"));
    }

    #[test]
    fn list_does_not_panic() {
        handle_list(&ListArgs {});
    }
}
