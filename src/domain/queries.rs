use std::path::PathBuf;

use crate::detection::finding::{FindingCategory, Severity};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindingsQuery {
    pub path: PathBuf,
    pub min_severity: Option<Severity>,
    pub categories: Vec<FindingCategory>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueStatusQuery {
    pub branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileListQuery {
    pub include_builtin: bool,
}

impl Default for ProfileListQuery {
    fn default() -> Self {
        Self {
            include_builtin: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSummaryQuery {
    pub path: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::ProfileListQuery;

    #[test]
    fn profile_list_query_defaults_to_including_builtins() {
        assert!(ProfileListQuery::default().include_builtin);
    }
}
