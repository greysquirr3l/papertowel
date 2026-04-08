mod learn;
mod profile;
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
}

#[derive(Debug, Args)]
struct LearnArgs {
    #[command(subcommand)]
    command: LearnCommand,
}

#[derive(Debug, Subcommand)]
enum LearnCommand {
    /// Analyse a repository and write a personalised style baseline.
    Repo(learn::LearnArgs),
    /// Display the existing style baseline for a repository.
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
        Command::Scrub(args) => scrub::handle(args),
        Command::Wring(args) => match args.command {
            WringCommand::Init(init_args) => wring::handle_init(init_args),
            WringCommand::Queue(queue_args) => wring::handle_queue(queue_args),
            WringCommand::Drip(drip_args) => wring::handle_drip(&drip_args),
            WringCommand::Status(status_args) => wring::handle_status(status_args),
        },
        Command::Learn(args) => match args.command {
            LearnCommand::Repo(repo_args) => learn::handle_learn(&repo_args),
            LearnCommand::Show(show_args) => learn::handle_show(&show_args),
        },
        Command::Clean(args) => {
            tracing::info!(path = %args.path, dry_run = args.dry_run, "clean placeholder");
            Ok(())
        }
        Command::Profile(args) => match args.command {
            ProfileCommand::Create(create_args) => profile::handle_create(create_args),
            ProfileCommand::List(list_args) => profile::handle_list(list_args),
            ProfileCommand::Show(show_args) => profile::handle_show(show_args),
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
}
