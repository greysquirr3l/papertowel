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

#[cfg(test)]
mod tests {
    use super::{CreateArgs, ListArgs, ShowArgs, handle_create, handle_list, handle_show};

    #[test]
    fn handle_create_returns_ok() {
        assert!(
            handle_create(CreateArgs {
                name: "night-owl".to_owned()
            })
            .is_ok()
        );
    }

    #[test]
    fn handle_list_returns_ok() {
        assert!(handle_list(ListArgs).is_ok());
    }

    #[test]
    fn handle_show_returns_ok() {
        assert!(
            handle_show(ShowArgs {
                name: "nine-to-five".to_owned()
            })
            .is_ok()
        );
    }
}
