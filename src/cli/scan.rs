use anyhow::Result;
use clap::Args;

use super::{OutputFormat, SeverityArg};

#[derive(Debug, Args)]
pub struct ScanArgs {
    pub path: String,
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
    #[arg(long, value_enum)]
    pub severity: Option<SeverityArg>,
}

pub fn handle(args: ScanArgs) -> Result<()> {
    tracing::info!(path = %args.path, format = ?args.format, severity = ?args.severity, "scan placeholder");
    Ok(())
}