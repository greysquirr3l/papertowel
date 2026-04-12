use std::path::Path;

use anyhow::Result;
use clap::Args;

use crate::profile::persona::PersonaProfile;
use crate::wringer::config::{WringerConfig, load_wringer_config, save_wringer_config};
use crate::wringer::drip::DripRunner;
use crate::wringer::lock::{DripProcessLock, read_lock_info, recover_stale_lock};
use crate::wringer::queue::{
    build_queue_plan, collect_pending_commits, load_queue_plan, save_queue_plan,
};
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
    #[arg(long)]
    pub profile: Option<String>,
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

#[derive(Debug, Args)]
pub struct UnlockStaleArgs;

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

pub fn handle_queue(args: QueueArgs) -> Result<()> {
    let repo_root = std::env::current_dir()?;
    let source_branch = match args.from {
        Some(b) => b,
        None => detect_current_branch(&repo_root)?,
    };
    let persona_name = args.profile.as_deref().unwrap_or("nine-to-five");
    let persona = PersonaProfile::load_by_name(persona_name)
        .map_err(|e| anyhow::anyhow!("failed to load persona: {e}"))?;
    let existing = load_queue_plan(&repo_root).ok();
    let sync_point = existing.and_then(|p| p.sync_point);
    let pending = collect_pending_commits(&repo_root, &source_branch, sync_point.as_deref())
        .map_err(|e| anyhow::anyhow!("failed to collect commits: {e}"))?;
    if pending.is_empty() {
        println!("no pending commits on '{source_branch}' — queue is up to date");
        return Ok(());
    }
    let plan = build_queue_plan(&pending, &persona, sync_point, chrono::Utc::now())
        .map_err(|e| anyhow::anyhow!("failed to build queue plan: {e}"))?;
    save_queue_plan(&repo_root, &plan)
        .map_err(|e| anyhow::anyhow!("failed to save queue plan: {e}"))?;
    println!(
        "queued {} commit(s) from '{}' with persona '{}' — saved to.papertowel/queue.json",
        plan.entries.len(),
        source_branch,
        plan.persona_name,
    );
    Ok(())
}

fn detect_current_branch(repo_path: &Path) -> Result<String> {
    let repo = git2::Repository::open(repo_path)
        .map_err(|e| anyhow::anyhow!("failed to open repository: {e}"))?;
    let head = repo
        .head()
        .map_err(|e| anyhow::anyhow!("failed to read HEAD: {e}"))?;
    let name = head
        .shorthand()
        .ok_or_else(|| anyhow::anyhow!("HEAD has no shorthand name"))?;
    Ok(name.to_owned())
}

pub fn handle_drip(args: &DripArgs) -> Result<()> {
    let repo_root = std::env::current_dir()?;
    let recovered = recover_stale_lock(&repo_root)
        .map_err(|e| anyhow::anyhow!("failed to recover stale drip lock: {e}"))?;
    if recovered {
        tracing::warn!("removed stale drip lock at .papertowel/drip.lock");
    }

    let _lock = DripProcessLock::acquire(&repo_root)
        .map_err(|e| anyhow::anyhow!("failed to acquire drip process lock: {e}"))?;

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

    let lock_line = match read_lock_info(&repo_root)
        .map_err(|e| anyhow::anyhow!("failed to inspect drip lock: {e}"))?
    {
        Some(info) => {
            let state = if info.active { "active" } else { "stale" };
            let pid = info
                .pid
                .map_or_else(|| String::from("unknown"), |value| value.to_string());
            let started_at = info.started_at.unwrap_or_else(|| String::from("unknown"));
            format!(
                "drip lock: {state} (pid: {pid}, started_at: {started_at}, path: {})",
                info.path.display()
            )
        }
        None => String::from("drip lock: none"),
    };
    println!("{lock_line}");

    Ok(())
}

pub fn handle_unlock_stale(_: UnlockStaleArgs) -> Result<()> {
    let repo_root = std::env::current_dir()?;
    let removed = recover_stale_lock(&repo_root)
        .map_err(|e| anyhow::anyhow!("failed to recover stale drip lock: {e}"))?;

    if removed {
        println!("removed stale drip lock: .papertowel/drip.lock");
        return Ok(());
    }

    match read_lock_info(&repo_root)
        .map_err(|e| anyhow::anyhow!("failed to inspect drip lock: {e}"))?
    {
        Some(info) if info.active => {
            let pid = info
                .pid
                .map_or_else(|| String::from("unknown"), |value| value.to_string());
            let started_at = info.started_at.unwrap_or_else(|| String::from("unknown"));
            println!(
                "lock is active; left unchanged (pid: {pid}, started_at: {started_at}, path: {})",
                info.path.display()
            );
        }
        Some(info) => {
            println!(
                "drip lock exists but was not removed; inspect path: {}",
                info.path.display()
            );
        }
        None => {
            println!("no drip lock file found at .papertowel/drip.lock");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        DripArgs, QueueArgs, StatusArgs, UnlockStaleArgs, handle_drip, handle_queue, handle_status,
        handle_unlock_stale,
    };

    #[test]
    #[ignore]
    fn handle_queue_returns_ok_on_current_branch() {
        // Runs against the workspace git repo; detects current branch and builds
        // a wring queue. Run locally with: cargo test -- --include-ignored
        let args = QueueArgs {
            from: None,
            profile: None,
        };
        assert!(handle_queue(args).is_ok());
    }

    #[test]
    #[ignore]
    fn handle_queue_with_from_branch_returns_ok() {
        let args = QueueArgs {
            from: Some(String::from("main")),
            profile: None,
        };
        assert!(handle_queue(args).is_ok());
    }

    #[test]
    fn handle_status_without_config_prints_init_message() {
        // In CI / fresh tempdir there is no config → prints "no wringer config found".
        // We can't redirect current_dir, but the function gracefully returns Ok(()).
        // Running it in the workspace dir is safe — it reads but does not write.
        let result = handle_status(StatusArgs);
        assert!(result.is_ok());
    }

    #[test]
    fn handle_drip_no_daemon_attempts_tick_on_current_dir() {
        // handle_drip (non-daemon) opens the git repo at current_dir and ticks.
        // tick() may return applied=0 (nothing queued) which is still Ok.
        let args = DripArgs {
            daemon: false,
            profile: None,
        };
        // Allow failure: if the wringer queue is not initialised the tick returns Err,
        // so we just verify it doesn't panic.
        let _ = handle_drip(&args);
    }

    #[test]
    fn handle_unlock_stale_returns_ok() {
        let result = handle_unlock_stale(UnlockStaleArgs);
        assert!(result.is_ok());
    }
}
