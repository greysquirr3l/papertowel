use std::collections::HashMap;
use std::io::{self, Write};

use serde::Serialize;

use crate::detection::finding::{Finding, FindingCategory, Severity};

// ─── Summary ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScanSummary {
    pub total_findings: usize,
    pub by_severity: HashMap<String, usize>,
    pub by_category: HashMap<String, usize>,
    /// Probability estimate that the code is AI-generated (0.0–1.0).
    pub ai_probability: f32,
}

#[expect(
    clippy::cast_precision_loss,
    reason = "bounded finding count: no meaningful precision loss"
)]
pub fn build_summary(findings: &[Finding]) -> ScanSummary {
    let mut by_severity: HashMap<String, usize> = HashMap::new();
    let mut by_category: HashMap<String, usize> = HashMap::new();
    let mut weighted_score = 0.0_f32;

    for f in findings {
        *by_severity.entry(severity_label(f.severity)).or_insert(0) += 1;
        *by_category.entry(category_label(f.category)).or_insert(0) += 1;

        let weight = match f.severity {
            Severity::High => 1.0_f32,
            Severity::Medium => 0.6,
            Severity::Low => 0.2,
        };
        weighted_score += weight * f.confidence_score;
    }

    // Normalise to 0.0–1.0 using a logistic curve
    let ai_probability = if findings.is_empty() {
        0.0_f32
    } else {
        let raw = weighted_score / findings.len() as f32;
        1.0 / (1.0 + (-8.0 * (raw - 0.5)).exp())
    };

    ScanSummary {
        total_findings: findings.len(),
        by_severity,
        by_category,
        ai_probability: ai_probability.clamp(0.0, 1.0),
    }
}

// ─── Text formatting ──────────────────────────────────────────────────────────

/// Write a human-readable scan report to `out`.  Findings are grouped by file
/// and sorted high→low within each group.
pub fn write_text_report(
    out: &mut impl Write,
    findings: &[Finding],
    summary: &ScanSummary,
) -> io::Result<()> {
    if findings.is_empty() {
        writeln!(out, "No findings.")?;
        writeln!(out)?;
        writeln!(
            out,
            "AI probability: {:.0}%",
            summary.ai_probability * 100.0
        )?;
        return Ok(());
    }

    // Group by file path, preserving insertion order via BTreeMap
    let mut by_file: std::collections::BTreeMap<String, Vec<&Finding>> =
        std::collections::BTreeMap::new();
    for f in findings {
        by_file
            .entry(f.file_path.to_string_lossy().into_owned())
            .or_default()
            .push(f);
    }

    for (file, mut group) in by_file {
        writeln!(out, "─── {file}")?;
        // Sort within the group: High > Medium > Low
        group.sort_by(|a, b| b.severity.cmp(&a.severity));
        for f in group {
            let loc = f
                .line_range
                .map_or_else(String::new, |r| format!(":{}", r.start));
            writeln!(
                out,
                "  [{sev}] {cat} — {desc}",
                sev = severity_label(f.severity),
                cat = category_label(f.category),
                desc = f.description,
            )?;
            if !loc.is_empty() {
                writeln!(out, "         at {file}{loc}")?;
            }
            if let Some(ref suggestion) = f.suggestion {
                writeln!(out, "         hint: {suggestion}")?;
            }
        }
        writeln!(out)?;
    }

    writeln!(out, "Summary")?;
    writeln!(out, "  Total findings : {}", summary.total_findings)?;
    for (sev, count) in &summary.by_severity {
        writeln!(out, "  {sev:8} : {count}")?;
    }
    writeln!(
        out,
        "AI probability : {:.0}%",
        summary.ai_probability * 100.0
    )?;

    Ok(())
}

// ─── JSON formatting ──────────────────────────────────────────────────────────

/// Write a structured JSON report to `out`.
pub fn write_json_report(
    out: &mut impl Write,
    findings: &[Finding],
    summary: &ScanSummary,
) -> io::Result<()> {
    #[derive(Serialize)]
    struct JsonReport<'a> {
        summary: &'a ScanSummary,
        findings: &'a [Finding],
    }

    let report = JsonReport { summary, findings };
    let json = serde_json::to_string_pretty(&report).map_err(io::Error::other)?;
    writeln!(out, "{json}")
}

// ─── GitHub Actions annotation formatting ─────────────────────────────────────

/// Write GitHub Actions workflow command annotations to `out`.
///
/// Each finding becomes an `::error` annotation that GitHub surfaces inline
/// in pull-request diffs and the Actions log viewer.  The format is:
/// `::error file={path},line={line},title={id}::{description}`
///
/// Findings with no line range omit the `line=` attribute so that GitHub
/// attaches the annotation to the file header rather than a specific line.
pub fn write_github_actions_report(
    out: &mut impl Write,
    findings: &[Finding],
    summary: &ScanSummary,
) -> io::Result<()> {
    for f in findings {
        let path = f.file_path.to_string_lossy();
        let title = format!("papertowel[{}]: {}", category_label(f.category), f.id);
        // Escape the message: `::` in the text would prematurely close the
        // command; newlines and percent signs also need escaping.
        let message = escape_gha_data(&f.description);

        if let Some(range) = f.line_range {
            writeln!(
                out,
                "::error file={path},line={line},title={title}::{message}",
                line = range.start,
            )?;
        } else {
            writeln!(out, "::error file={path},title={title}::{message}")?;
        }
    }

    // Emit a summary notice after all annotations.
    let ai_pct = summary.ai_probability * 100.0;
    writeln!(
        out,
        "::notice title=papertowel summary::{total} finding(s) \u{2014} AI probability {ai_pct:.0}%",
        total = summary.total_findings,
    )?;

    Ok(())
}

/// Escape a string for use as the data portion of a GitHub Actions workflow
/// command (`::command key=value::data`).
fn escape_gha_data(s: &str) -> String {
    // GitHub Actions command data escaping: percent, carriage-return, newline.
    s.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn severity_label(s: Severity) -> String {
    match s {
        Severity::High => "HIGH".to_owned(),
        Severity::Medium => "MED".to_owned(),
        Severity::Low => "LOW".to_owned(),
    }
}

fn category_label(c: FindingCategory) -> String {
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
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::path::PathBuf;

    use super::{build_summary, write_github_actions_report, write_json_report, write_text_report};
    use crate::detection::finding::{Finding, FindingCategory, Severity};

    fn make_finding(sev: Severity, path: &str) -> Finding {
        Finding::new(
            "test.id",
            FindingCategory::Lexical,
            sev,
            0.8,
            PathBuf::from(path),
            "slop word detected",
        )
        .expect("valid finding")
    }

    #[test]
    fn empty_findings_produces_no_finding_output() {
        let summary = build_summary(&[]);
        let mut out = Vec::new();
        write_text_report(&mut out, &[], &summary).expect("write");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains("No findings."));
    }

    #[test]
    fn text_report_groups_by_file() {
        let findings = vec![
            make_finding(Severity::High, "src/main.rs"),
            make_finding(Severity::Low, "src/lib.rs"),
            make_finding(Severity::Medium, "src/main.rs"),
        ];
        let summary = build_summary(&findings);
        let mut out = Vec::new();
        write_text_report(&mut out, &findings, &summary).expect("write");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains("src/main.rs"));
        assert!(text.contains("src/lib.rs"));
        assert!(text.contains("[HIGH]"));
    }

    #[test]
    fn json_report_is_valid_json() {
        let findings = vec![make_finding(Severity::Medium, "src/lib.rs")];
        let summary = build_summary(&findings);
        let mut out = Vec::new();
        write_json_report(&mut out, &findings, &summary).expect("write");
        let text = String::from_utf8(out).expect("utf8");
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert!(parsed.get("findings").is_some());
        assert!(parsed.get("summary").is_some());
    }

    #[test]
    fn build_summary_counts_correctly() {
        let findings = vec![
            make_finding(Severity::High, "a.rs"),
            make_finding(Severity::High, "b.rs"),
            make_finding(Severity::Medium, "c.rs"),
        ];
        let summary = build_summary(&findings);
        assert_eq!(summary.total_findings, 3);
        assert_eq!(summary.by_severity.get("HIGH"), Some(&2));
        assert_eq!(summary.by_severity.get("MED"), Some(&1));
    }

    #[test]
    fn ai_probability_is_higher_for_more_findings() {
        let few = vec![make_finding(Severity::Low, "a.rs")];
        let many: Vec<_> = (0..10)
            .map(|i| make_finding(Severity::High, &format!("{i}.rs")))
            .collect();
        let p_few = build_summary(&few).ai_probability;
        let p_many = build_summary(&many).ai_probability;
        assert!(
            p_many > p_few,
            "more high-severity findings → higher probability"
        );
    }

    #[test]
    fn github_actions_report_emits_error_annotations() {
        let findings = vec![make_finding(Severity::High, "src/main.rs")];
        let summary = build_summary(&findings);
        let mut out = Vec::new();
        write_github_actions_report(&mut out, &findings, &summary).expect("write");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains("::error file=src/main.rs"), "expected ::error annotation");
        assert!(text.contains("::notice title=papertowel summary::"), "expected summary notice");
    }

    #[test]
    fn gha_report_escapes_percent_in_description() {
        let mut f = make_finding(Severity::Medium, "src/lib.rs");
        f.description = "100% AI-generated".to_owned();
        let summary = build_summary(&[f.clone()]);
        let mut out = Vec::new();
        write_github_actions_report(&mut out, &[f], &summary).expect("write");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains("100%25 AI-generated"), "percent must be escaped");
    }
}
