use crate::detection::finding::{FindingCategory, Severity};

pub(super) fn severity_label(s: Severity) -> String {
    match s {
        Severity::High => "HIGH".to_owned(),
        Severity::Medium => "MED".to_owned(),
        Severity::Low => "LOW".to_owned(),
    }
}

pub(super) fn category_label(c: FindingCategory) -> String {
    match c {
        FindingCategory::Lexical => "lexical".to_owned(),
        FindingCategory::Comment => "comment".to_owned(),
        FindingCategory::Structure => "structure".to_owned(),
        FindingCategory::Readme => "readme".to_owned(),
        FindingCategory::Metadata => "metadata".to_owned(),
        FindingCategory::Workflow => "workflow".to_owned(),
        FindingCategory::Maintenance => "maintenance".to_owned(),
        FindingCategory::Promotion => "promotion".to_owned(),
        FindingCategory::NameCredibility => "name_credibility".to_owned(),
        FindingCategory::IdiomMismatch => "idiom_mismatch".to_owned(),
        FindingCategory::TestPattern => "test_pattern".to_owned(),
        FindingCategory::PromptLeakage => "prompt_leakage".to_owned(),
        FindingCategory::CommitPattern => "commit_pattern".to_owned(),
        FindingCategory::Architecture => "architecture".to_owned(),
        FindingCategory::Security => "security".to_owned(),
        FindingCategory::InvisibleUnicode => "invisible_unicode".to_owned(),
    }
}
