use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
pub struct InitArgs {
    #[arg(long)]
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

#[expect(clippy::needless_pass_by_value, clippy::unnecessary_wraps)]
pub fn handle_init(args: InitArgs) -> Result<()> {
    tracing::info!(branch = ?args.branch, "wring init placeholder");
    Ok(())
}

#[expect(clippy::needless_pass_by_value, clippy::unnecessary_wraps)]
pub fn handle_queue(args: QueueArgs) -> Result<()> {
    tracing::info!(from = ?args.from, "wring queue placeholder");
    Ok(())
}

#[expect(clippy::needless_pass_by_value, clippy::unnecessary_wraps)]
pub fn handle_drip(args: DripArgs) -> Result<()> {
    tracing::info!(daemon = args.daemon, profile = ?args.profile, "wring drip placeholder");
    Ok(())
}

#[expect(clippy::unnecessary_wraps)]
pub fn handle_status(_: StatusArgs) -> Result<()> {
    tracing::info!("wring status placeholder");
    Ok(())
}