use anyhow::Result;
use clap::Args;

use crate::wringer::config::{WringerConfig, load_wringer_config, save_wringer_config};
use crate::wringer::drip::DripRunner;
use crate::wringer::worktree::{WorktreeSpec, initialize_worktree, status_worktree};

#[derive(Debug, Args)]
pub struct InitArgs {
    #[arg(long, default_value = "public")]
    pub branch: Option<String>,
}

#[derive(Debug, Args)]
pub struct QueueArgs {
    #[arg(long)]
    pub from: Option<String>,
}

#[derive(Debug, Args)]
pub struct DripArgs {
    #[arg(long)]
    pub daemon: bool,
    #[arg(long)]
    pub profile: Option<String>,
}

#[derive(Debug, Args)]
pub struct StatusArgs;

pub fn handle_init(args: InitArgs) -> Result<()> {
    let repo_root = std::env::current_dir()?;
    let branch = args.branch.unwrap_or_else(|| String::from("public"));
    let worktree_path = repo_root.join("..").join(format!(
        "{}-public",
        repo_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("repo")
    ));
    let worktree_name = branch.clone();

    let spec = WorktreeSpec {
        name: worktree_name.clone(),
        branch: branch.clone(),
        path: worktree_path.clone(),
    };

    let status = initialize_worktree(&repo_root, &spec)
        .map_err(|error| anyhow::anyhow!("failed to initialize worktree: {error}"))?;

    let config = WringerConfig {
        branch,
        worktree_path,
        worktree_name,
    };
    save_wringer_config(&repo_root, &config)
        .map_err(|error| anyhow::anyhow!("failed to save wringer config: {error}"))?;

    if status.exists {
        println!(
            "worktree '{}' ready at {}",
            status.name,
            status.path.display()
        );
    } else {
        println!("worktree '{}' not found after initialization", status.name);
    }
    Ok(())
}

#[expect(clippy::needless_pass_by_value, clippy::unnecessary_wraps)]
pub fn handle_queue(args: QueueArgs) -> Result<()> {
    tracing::info!(from = ?args.from, "wring queue placeholder");
    Ok(())
}

pub fn handle_drip(args: &DripArgs) -> Result<()> {
    let repo_root = std::env::current_dir()?;
    let mut runner = DripRunner::new(&repo_root)
        .map_err(|e| anyhow::anyhow!("failed to initialise drip runner: {e}"))?;

    if args.daemon {
        tracing::info!("entering daemon mode — polling every 60 s");
        loop {
            let stats = runner
                .tick()
                .map_err(|e| anyhow::anyhow!("drip tick failed: {e}"))?;
            if stats.applied > 0 {
                println!(
                    "applied {} commit(s), {} pending, {} already done",
                    stats.applied, stats.pending, stats.already_done
                );
            }
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
    } else {
        let stats = runner
            .tick()
            .map_err(|e| anyhow::anyhow!("drip tick failed: {e}"))?;
        println!(
            "applied {} commit(s), {} pending, {} already done",
            stats.applied, stats.pending, stats.already_done
        );
        Ok(())
    }
}

pub fn handle_status(_: StatusArgs) -> Result<()> {
    let repo_root = std::env::current_dir()?;

    let Ok(config) = load_wringer_config(&repo_root) else {
        println!("no wringer config found — run `papertowel wring init` first");
        return Ok(());
    };

    let spec = config.to_spec();
    let status = status_worktree(&repo_root, &spec)
        .map_err(|error| anyhow::anyhow!("failed to query worktree status: {error}"))?;

    if status.exists {
        println!(
            "worktree '{}' exists at {} (branch: {})",
            status.name,
            status.path.display(),
            status.branch
        );
    } else {
        println!(
            "worktree '{}' not found (expected at {})",
            status.name,
            status.path.display()
        );
    }
    Ok(())
}
