use anyhow::Result;
use tracing_subscriber::EnvFilter;

fn init_tracing() -> Result<()> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .try_init()
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    Ok(())
}

fn main() -> Result<()> {
    init_tracing()?;
    papertowel::cli::run()
}
