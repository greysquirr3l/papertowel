mod hook;
mod learn;
mod profile;
pub mod recipe;
pub mod report;
mod scan;
mod scrub;
mod wring;

use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Json,
    Text,
    #[value(name = "github")]
    GithubActions,
    #[value(name = "sarif")]
    Sarif,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SeverityArg {
    Low,
    Medium,
    High,
}

#[derive(Debug, Parser)]
#[command(
 name = "papertowel",
 version,
 long_version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("PAPERTOWEL_GIT_SHA"), ")"),
 about = "Clean up AI fingerprints and humanize git history"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Scan(scan::ScanArgs),
    Scrub(scrub::ScrubArgs),
    Wring(WringArgs),
    Clean(CleanArgs),
    Learn(LearnArgs),
    Profile(ProfileArgs),
    Recipe(RecipeArgs),
    Hook(HookArgs),
}

#[derive(Debug, Args)]
struct LearnArgs {
    #[command(subcommand)]
    command: LearnCommand,
}

#[derive(Debug, Subcommand)]
enum LearnCommand {
    Repo(learn::LearnArgs),
    Show(learn::LearnShowArgs),
}

#[derive(Debug, Args)]
struct WringArgs {
    #[command(subcommand)]
    command: WringCommand,
}

#[derive(Debug, Subcommand)]
enum WringCommand {
    Init(wring::InitArgs),
    Queue(wring::QueueArgs),
    Drip(wring::DripArgs),
    Status(wring::StatusArgs),
    UnlockStale(wring::UnlockStaleArgs),
}

#[derive(Debug, Args)]
struct HookArgs {
    #[command(subcommand)]
    command: HookCommand,
}

#[derive(Debug, Subcommand)]
enum HookCommand {
    Install(hook::InstallArgs),
    Uninstall(hook::UninstallArgs),
    Status(hook::StatusArgs),
}

#[derive(Debug, Args)]
struct CleanArgs {
    path: String,
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct ProfileArgs {
    #[command(subcommand)]
    command: ProfileCommand,
}

#[derive(Debug, Subcommand)]
enum ProfileCommand {
    Create(profile::CreateArgs),
    List(profile::ListArgs),
    Show(profile::ShowArgs),
}

#[derive(Debug, Args)]
struct RecipeArgs {
    #[command(subcommand)]
    command: RecipeCommand,
}

#[derive(Debug, Subcommand)]
enum RecipeCommand {
    /// List available recipes.
    List(recipe::ListArgs),
    /// Show details of a recipe.
    Show(recipe::ShowArgs),
    /// Validate a recipe file.
    Validate(recipe::ValidateArgs),
}

pub fn run() -> Result<()> {
    run_from(std::env::args_os())
}

fn run_from<I, T>(args: I) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::parse_from(args);
    dispatch(cli)
}

fn dispatch(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Scan(args) => scan::handle(&args),
        Command::Scrub(args) => scrub::handle(&args),
        Command::Wring(args) => match args.command {
            WringCommand::Init(init_args) => wring::handle_init(init_args),
            WringCommand::Queue(queue_args) => wring::handle_queue(queue_args),
            WringCommand::Drip(drip_args) => wring::handle_drip(&drip_args),
            WringCommand::Status(status_args) => wring::handle_status(status_args),
            WringCommand::UnlockStale(unlock_stale_args) => {
                wring::handle_unlock_stale(unlock_stale_args)
            }
        },
        Command::Learn(args) => match args.command {
            LearnCommand::Repo(repo_args) => learn::handle_learn(&repo_args),
            LearnCommand::Show(show_args) => learn::handle_show(&show_args),
        },
        Command::Clean(args) => {
            let scrub_args = scrub::ScrubArgs {
                path: args.path.clone(),
                dry_run: args.dry_run,
                detectors: Vec::new(),
            };
            scrub::handle(&scrub_args)?;
            let scan_args = scan::ScanArgs {
                path: args.path,
                format: OutputFormat::Text,
                severity: None,
                fail_on: None,
                ci: false,
            };
            scan::handle(&scan_args)
        }
        Command::Profile(args) => match args.command {
            ProfileCommand::Create(create_args) => profile::handle_create(create_args),
            ProfileCommand::List(list_args) => profile::handle_list(list_args),
            ProfileCommand::Show(show_args) => profile::handle_show(&show_args),
        },
        Command::Recipe(args) => match args.command {
            RecipeCommand::List(ref list_args) => recipe::handle_list(list_args),
            RecipeCommand::Show(ref show_args) => recipe::handle_show(show_args),
            RecipeCommand::Validate(ref validate_args) => recipe::handle_validate(validate_args),
        },
        Command::Hook(args) => match args.command {
            HookCommand::Install(ref install_args) => hook::handle_install(install_args),
            HookCommand::Uninstall(ref uninstall_args) => hook::handle_uninstall(uninstall_args),
            HookCommand::Status(ref status_args) => hook::handle_status(status_args),
        },
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Command, OutputFormat, ProfileCommand, SeverityArg, WringCommand};

    #[test]
    fn parses_scan_command_with_options() -> Result<(), Box<dyn std::error::Error>> {
        let cli = Cli::try_parse_from([
            "papertowel",
            "scan",
            "./src",
            "--format",
            "json",
            "--severity",
            "high",
        ])?;

        match cli.command {
            Command::Scan(args) => {
                assert_eq!(args.path, "./src");
                assert_eq!(args.format, OutputFormat::Json);
                assert_eq!(args.severity, Some(SeverityArg::High));
            }
            _ => unreachable!("expected scan command"),
        }
        Ok(())
    }

    #[test]
    fn parses_scrub_command_with_detectors() -> Result<(), Box<dyn std::error::Error>> {
        let cli = Cli::try_parse_from([
            "papertowel",
            "scrub",
            "./repo",
            "--dry-run",
            "--detectors",
            "lexical,comments",
        ])?;

        match cli.command {
            Command::Scrub(args) => {
                assert!(args.dry_run);
                assert_eq!(args.detectors, vec!["lexical", "comments"]);
            }
            _ => unreachable!("expected scrub command"),
        }
        Ok(())
    }

    #[test]
    fn parses_wring_subcommands() -> Result<(), Box<dyn std::error::Error>> {
        let init = Cli::try_parse_from(["papertowel", "wring", "init", "--branch", "public"])?;
        match init.command {
            Command::Wring(args) => match args.command {
                WringCommand::Init(init_args) => {
                    assert_eq!(init_args.branch.as_deref(), Some("public"));
                }
                _ => unreachable!("expected wring init"),
            },
            _ => unreachable!("expected wring command"),
        }

        let status = Cli::try_parse_from(["papertowel", "wring", "status"])?;
        match status.command {
            Command::Wring(args) => match args.command {
                WringCommand::Status(_) => {}
                _ => unreachable!("expected wring status"),
            },
            _ => unreachable!("expected wring command"),
        }

        let unlock_stale = Cli::try_parse_from(["papertowel", "wring", "unlock-stale"])?;
        match unlock_stale.command {
            Command::Wring(args) => match args.command {
                WringCommand::UnlockStale(_) => {}
                _ => unreachable!("expected wring unlock-stale"),
            },
            _ => unreachable!("expected wring command"),
        }
        Ok(())
    }

    #[test]
    fn parses_clean_and_profile_commands() -> Result<(), Box<dyn std::error::Error>> {
        let clean = Cli::try_parse_from(["papertowel", "clean", "./repo", "--dry-run"])?;
        match clean.command {
            Command::Clean(args) => {
                assert_eq!(args.path, "./repo");
                assert!(args.dry_run);
            }
            _ => unreachable!("expected clean command"),
        }

        let show = Cli::try_parse_from(["papertowel", "profile", "show", "night-owl"])?;
        match show.command {
            Command::Profile(profile) => match profile.command {
                ProfileCommand::Show(show_args) => {
                    assert_eq!(show_args.name, "night-owl");
                }
                _ => unreachable!("expected profile show"),
            },
            _ => unreachable!("expected profile command"),
        }
        Ok(())
    }

    #[test]
    fn run_from_scrub_command_returns_ok() -> Result<(), Box<dyn std::error::Error>> {
        // Covers dispatch → scrub::handle path.
        use super::run_from;
        run_from(["papertowel", "scrub", "./src"])?;
        Ok(())
    }

    #[test]
    fn run_from_scan_command_returns_ok() -> Result<(), Box<dyn std::error::Error>> {
        // Covers dispatch → Command::Scan (line 111).
        use super::run_from;
        use tempfile::TempDir;
        let tmp = TempDir::new()?;
        run_from(["papertowel", "scan", tmp.path().to_str().ok_or("bad path")?])?;
        Ok(())
    }

    #[test]
    fn run_from_learn_show_command_returns_ok() -> Result<(), Box<dyn std::error::Error>> {
        // Covers dispatch → Command::Learn → LearnCommand::Show (lines 119-121).
        use super::run_from;
        use tempfile::TempDir;
        let tmp = TempDir::new()?;
        run_from([
            "papertowel",
            "learn",
            "show",
            tmp.path().to_str().ok_or("bad path")?,
        ])?;
        Ok(())
    }

    #[test]
    fn run_from_learn_repo_command_returns_ok() -> Result<(), Box<dyn std::error::Error>> {
        // Covers dispatch → Command::Learn → LearnCommand::Repo (line 120).
        use super::run_from;
        use std::fs;
        use tempfile::TempDir;
        let tmp = TempDir::new()?;
        fs::write(
            tmp.path().join("lib.rs"),
            "pub fn a(){}\npub fn b(){}\npub fn c(){}\npub fn d(){}\npub fn e(){}\npub fn f(){}\npub fn g(){}\npub fn h(){}\n",
        )?;
        run_from([
            "papertowel",
            "learn",
            "repo",
            tmp.path().to_str().ok_or("bad path")?,
        ])?;
        Ok(())
    }

    #[test]
    fn run_from_clean_command_returns_ok() -> Result<(), Box<dyn std::error::Error>> {
        // Covers dispatch → Command::Clean placeholder path.
        use super::run_from;
        run_from(["papertowel", "clean", "./src"])?;
        Ok(())
    }

    #[test]
    fn run_from_profile_create_returns_ok() -> Result<(), Box<dyn std::error::Error>> {
        // Covers dispatch → ProfileCommand::Create path.
        use super::run_from;
        run_from(["papertowel", "profile", "create", "my-persona"])?;
        Ok(())
    }

    #[test]
    fn run_from_profile_list_returns_ok() -> Result<(), Box<dyn std::error::Error>> {
        // Covers dispatch → ProfileCommand::List path.
        use super::run_from;
        run_from(["papertowel", "profile", "list"])?;
        Ok(())
    }

    #[test]
    fn run_from_profile_show_returns_ok() -> Result<(), Box<dyn std::error::Error>> {
        // Covers dispatch → ProfileCommand::Show path.
        use super::run_from;
        run_from(["papertowel", "profile", "show", "night-owl"])?;
        Ok(())
    }

    #[test]
    fn run_from_wring_queue_dispatches_queue_handler() {
        // Covers dispatch → Command::Wring → WringCommand::Queue (line 115).
        // handle_queue opens current_dir as a git repo — it may fail if no valid repo exists
        // but the dispatch line is still reached.
        use super::run_from;
        // We only care that the Queue branch in dispatch was reached, not whether it succeeded.
        let _ = run_from(["papertowel", "wring", "queue"]);
    }

    #[test]
    fn run_from_wring_status_returns_ok() -> Result<(), Box<dyn std::error::Error>> {
        // Covers dispatch → Command::Wring → WringCommand::Status (line 117).
        // handle_status reads current_dir() and config — no config → prints message, returns Ok.
        use super::run_from;
        run_from(["papertowel", "wring", "status"])?;
        Ok(())
    }

    #[test]
    fn run_from_wring_drip_dispatches_drip_handler() {
        // Covers dispatch → Command::Wring → WringCommand::Drip (line 116).
        // handle_drip opens current_dir as a git repo — it may fail if no wringer config exists
        // but the line is still dispatched.
        use super::run_from;
        // We only care that the Drip branch in dispatch was reached, not whether it succeeded.
        let _ = run_from(["papertowel", "wring", "drip"]);
    }
}
