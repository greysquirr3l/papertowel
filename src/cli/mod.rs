mod profile;
mod scan;
mod scrub;
mod wring;

use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Json,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SeverityArg {
    Low,
    Medium,
    High,
}

#[derive(Debug, Parser)]
#[command(name = "papertowel", version, about = "Clean up AI fingerprints and humanize git history")]
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
    Profile(ProfileArgs),
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
        Command::Scan(args) => scan::handle(args),
        Command::Scrub(args) => scrub::handle(args),
        Command::Wring(args) => match args.command {
            WringCommand::Init(init_args) => wring::handle_init(init_args),
            WringCommand::Queue(queue_args) => wring::handle_queue(queue_args),
            WringCommand::Drip(drip_args) => wring::handle_drip(drip_args),
            WringCommand::Status(status_args) => wring::handle_status(status_args),
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
    fn parses_scan_command_with_options() {
        let cli = Cli::try_parse_from([
            "papertowel",
            "scan",
            "./src",
            "--format",
            "json",
            "--severity",
            "high",
        ]);
        assert!(cli.is_ok());

        let cli = match cli {
            Ok(cli) => cli,
            Err(error) => panic!("unexpected parse error: {error}"),
        };

        match cli.command {
            Command::Scan(args) => {
                assert_eq!(args.path, "./src");
                assert_eq!(args.format, OutputFormat::Json);
                assert_eq!(args.severity, Some(SeverityArg::High));
            }
            _ => panic!("expected scan command"),
        }
    }

    #[test]
    fn parses_scrub_command_with_detectors() {
        let cli = Cli::try_parse_from([
            "papertowel",
            "scrub",
            "./repo",
            "--dry-run",
            "--detectors",
            "lexical,comments",
        ]);
        assert!(cli.is_ok());

        let cli = match cli {
            Ok(cli) => cli,
            Err(error) => panic!("unexpected parse error: {error}"),
        };

        match cli.command {
            Command::Scrub(args) => {
                assert!(args.dry_run);
                assert_eq!(args.detectors, vec!["lexical", "comments"]);
            }
            _ => panic!("expected scrub command"),
        }
    }

    #[test]
    fn parses_wring_subcommands() {
        let init = Cli::try_parse_from(["papertowel", "wring", "init", "--branch", "public"]);
        assert!(init.is_ok());
        let init = match init {
            Ok(cli) => cli,
            Err(error) => panic!("unexpected parse error: {error}"),
        };
        match init.command {
            Command::Wring(args) => match args.command {
                WringCommand::Init(init_args) => {
                    assert_eq!(init_args.branch.as_deref(), Some("public"));
                }
                _ => panic!("expected wring init"),
            },
            _ => panic!("expected wring command"),
        }

        let status = Cli::try_parse_from(["papertowel", "wring", "status"]);
        assert!(status.is_ok());
        let status = match status {
            Ok(cli) => cli,
            Err(error) => panic!("unexpected parse error: {error}"),
        };
        match status.command {
            Command::Wring(args) => match args.command {
                WringCommand::Status(_) => {}
                _ => panic!("expected wring status"),
            },
            _ => panic!("expected wring command"),
        }
    }

    #[test]
    fn parses_clean_and_profile_commands() {
        let clean = Cli::try_parse_from(["papertowel", "clean", "./repo", "--dry-run"]);
        assert!(clean.is_ok());
        let clean = match clean {
            Ok(cli) => cli,
            Err(error) => panic!("unexpected parse error: {error}"),
        };
        match clean.command {
            Command::Clean(args) => {
                assert_eq!(args.path, "./repo");
                assert!(args.dry_run);
            }
            _ => panic!("expected clean command"),
        }

        let show = Cli::try_parse_from(["papertowel", "profile", "show", "night-owl"]);
        assert!(show.is_ok());
        let show = match show {
            Ok(cli) => cli,
            Err(error) => panic!("unexpected parse error: {error}"),
        };
        match show.command {
            Command::Profile(profile) => match profile.command {
                ProfileCommand::Show(show_args) => {
                    assert_eq!(show_args.name, "night-owl");
                }
                _ => panic!("expected profile show"),
            },
            _ => panic!("expected profile command"),
        }
    }
}