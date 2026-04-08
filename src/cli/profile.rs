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

pub fn handle_create(args: CreateArgs) -> Result<()> {
    tracing::info!(name = %args.name, "profile create placeholder");
    Ok(())
}

pub fn handle_list(_: ListArgs) -> Result<()> {
    tracing::info!("profile list placeholder");
    Ok(())
}

pub fn handle_show(args: ShowArgs) -> Result<()> {
    tracing::info!(name = %args.name, "profile show placeholder");
    Ok(())
}