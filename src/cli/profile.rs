use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
pub struct CreateArgs {
    pub name: String,
}

#[derive(Debug, Args)]
pub struct ListArgs;

#[derive(Debug, Args)]
pub struct ShowArgs {
    pub name: String,
}

#[expect(clippy::needless_pass_by_value, clippy::unnecessary_wraps)]
pub fn handle_create(args: CreateArgs) -> Result<()> {
    tracing::info!(name = %args.name, "profile create placeholder");
    Ok(())
}

#[expect(clippy::unnecessary_wraps)]
pub fn handle_list(_: ListArgs) -> Result<()> {
    tracing::info!("profile list placeholder");
    Ok(())
}

#[expect(clippy::needless_pass_by_value, clippy::unnecessary_wraps)]
pub fn handle_show(args: ShowArgs) -> Result<()> {
    tracing::info!(name = %args.name, "profile show placeholder");
    Ok(())
}
