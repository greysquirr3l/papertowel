mod profile;
mod scan;
mod scrub;
mod wring;

use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Json,
    Text,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
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
    let cli = Cli::parse();

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