use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FindingCategory {
    Lexical,
    Comment,
    Structure,
    Readme,
    Metadata,
    Workflow,
    Maintenance,
    Promotion,
    NameCredibility,
    IdiomMismatch,
    TestPattern,
    PromptLeakage,
    CommitPattern,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Severity {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub category: FindingCategory,
    pub severity: Severity,
    pub confidence_score: f32,
    pub file_path: String,
    pub line_range: Option<LineRange>,
    pub description: String,
    pub suggestion: Option<String>,
    pub auto_fixable: bool,
}