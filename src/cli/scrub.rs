use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
pub struct ScrubArgs {
    pub path: String,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long, value_delimiter = ',')]
    pub detectors: Vec<String>,
}

pub fn handle(args: ScrubArgs) -> Result<()> {
    tracing::info!(path = %args.path, dry_run = args.dry_run, detectors = ?args.detectors, "scrub placeholder");
    Ok(())
}