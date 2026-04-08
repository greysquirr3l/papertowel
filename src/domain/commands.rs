use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SeverityThreshold {
    Low,
    #[default]
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanCommand {
    pub path: PathBuf,
    pub format: OutputFormat,
    pub min_severity: SeverityThreshold,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrubCommand {
    pub path: PathBuf,
    pub dry_run: bool,
    pub detectors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WringInitCommand {
    pub branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WringQueueCommand {
    pub from_branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WringDripCommand {
    pub daemon: bool,
    pub profile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WringStatusCommand;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanCommand {
    pub path: PathBuf,
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileCreateCommand {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileShowCommand {
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::{OutputFormat, SeverityThreshold};

    #[test]
    fn output_format_defaults_to_text() {
        assert_eq!(OutputFormat::default(), OutputFormat::Text);
    }

    #[test]
    fn severity_threshold_defaults_to_medium() {
        assert_eq!(SeverityThreshold::default(), SeverityThreshold::Medium);
    }
}