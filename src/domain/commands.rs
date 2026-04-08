#[derive(Debug, Clone)]
pub struct ScanCommand {
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct ScrubCommand {
    pub path: String,
    pub dry_run: bool,
}

#[derive(Debug, Clone)]
pub struct WringInitCommand {
    pub branch: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WringQueueCommand {
    pub from: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WringDripCommand {
    pub daemon: bool,
    pub profile: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProfileCreateCommand {
    pub name: String,
}