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

/// ANSI colour helpers — all no-ops when `use_color` is false.
struct Ansi {
    use_color: bool,
}

impl Ansi {
    const RESET: &'static str = "\x1b[0m";
    const BOLD: &'static str = "\x1b[1m";
    const DIM: &'static str = "\x1b[2m";
    const RED: &'static str = "\x1b[31m";
    const YELLOW: &'static str = "\x1b[33m";
    const CYAN: &'static str = "\x1b[36m";
    const GREEN: &'static str = "\x1b[32m";

    fn wrap<'a>(&self, codes: &'static str, s: &'a str) -> std::borrow::Cow<'a, str> {
        if self.use_color {
            format!("{codes}{s}{reset}", reset = Self::RESET).into()
        } else {
            s.into()
        }
    }

    fn bold<'a>(&self, s: &'a str) -> std::borrow::Cow<'a, str> {
        self.wrap(Self::BOLD, s)
    }

    fn dim<'a>(&self, s: &'a str) -> std::borrow::Cow<'a, str> {
        self.wrap(Self::DIM, s)
    }

    fn severity_badge(&self, s: Severity) -> String {
        let label = match s {
            Severity::High => "HIGH",
            Severity::Medium => " MED",
            Severity::Low => " LOW",
        };
        if self.use_color {
            let color = match s {
                Severity::High => Self::RED,
                Severity::Medium => Self::YELLOW,
                Severity::Low => Self::CYAN,
            };
            format!("{}{}{}{}", Self::BOLD, color, label, Self::RESET)
        } else {
            label.to_owned()
        }
    }

    fn ai_prob_color(&self, prob: f32) -> &'static str {
        if !self.use_color {
            return "";
        }
        if prob >= 0.75 {
            Self::RED
        } else if prob >= 0.50 {
            Self::YELLOW
        } else {
            Self::GREEN
        }
    }
}

/// Write a human-readable scan report to `out`.
///
/// Pass `use_color = true` when the sink is an interactive terminal.
/// Findings are grouped by file and sorted high→low within each group.
pub fn write_text_report(
    out: &mut impl Write,
    findings: &[Finding],
    summary: &ScanSummary,
    use_color: bool,
) -> io::Result<()> {
    let a = Ansi { use_color };

    if findings.is_empty() {
        let pct = summary.ai_probability * 100.0;
        let color = a.ai_prob_color(summary.ai_probability);
        if use_color {
            writeln!(
                out,
                "{}No findings.{}  AI likelihood {}{}{:.0}%{}",
                Ansi::BOLD,
                Ansi::RESET,
                color,
                Ansi::BOLD,
                pct,
                Ansi::RESET
            )?;
        } else {
            writeln!(out, "No findings.  AI likelihood {pct:.0}%")?;
        }
        return Ok(());
    }

    write_text_findings(out, findings, &a)?;
    write_text_summary(out, summary, &a)?;
    writeln!(out, "{}", a.dim(&"─".repeat(52)))?;

    Ok(())
}

fn write_text_findings(out: &mut impl Write, findings: &[Finding], a: &Ansi) -> io::Result<()> {
    let mut by_file: std::collections::BTreeMap<String, Vec<&Finding>> =
        std::collections::BTreeMap::new();
    for f in findings {
        by_file
            .entry(f.file_path.to_string_lossy().into_owned())
            .or_default()
            .push(f);
    }
    for (file, mut group) in by_file {
        let display_file = if file == "." {
            "(repo root)".to_owned()
        } else {
            file.clone()
        };
        writeln!(out, "{}", a.bold(&display_file))?;
        group.sort_by(|x, y| y.severity.cmp(&x.severity));
        for f in group {
            let badge = a.severity_badge(f.severity);
            let cat = category_label(f.category);
            writeln!(out, "  [{badge}] {cat} — {desc}", desc = f.description)?;
            if let Some(range) = f.line_range {
                writeln!(
                    out,
                    "         {}",
                    a.dim(&format!("at {file}:{}", range.start))
                )?;
            }
            if let Some(ref suggestion) = f.suggestion {
                writeln!(out, "         {} {suggestion}", a.dim("→"))?;
            }
        }
        writeln!(out)?;
    }
    Ok(())
}

fn write_text_summary(out: &mut impl Write, summary: &ScanSummary, a: &Ansi) -> io::Result<()> {
    let high = summary.by_severity.get("HIGH").copied().unwrap_or(0);
    let med = summary.by_severity.get("MED").copied().unwrap_or(0);
    let low = summary.by_severity.get("LOW").copied().unwrap_or(0);
    let pct = summary.ai_probability * 100.0;
    let pct_color = a.ai_prob_color(summary.ai_probability);

    writeln!(out, "{}", a.dim(&"─".repeat(52)))?;

    if a.use_color {
        write!(
            out,
            "  {}{} findings{}  {}  ",
            Ansi::BOLD,
            summary.total_findings,
            Ansi::RESET,
            a.dim("·"),
        )?;
        if high > 0 {
            write!(
                out,
                "{}{}HIGH {high}{}  ",
                Ansi::BOLD,
                Ansi::RED,
                Ansi::RESET
            )?;
        }
        if med > 0 {
            write!(
                out,
                "{}{}MED {med}{}  ",
                Ansi::BOLD,
                Ansi::YELLOW,
                Ansi::RESET
            )?;
        }
        if low > 0 {
            write!(
                out,
                "{}{}LOW {low}{}  ",
                Ansi::BOLD,
                Ansi::CYAN,
                Ansi::RESET
            )?;
        }
        writeln!(
            out,
            "{}  AI likelihood {}{}{pct:.0}%{}",
            a.dim("·"),
            pct_color,
            Ansi::BOLD,
            Ansi::RESET
        )?;
    } else {
        write!(out, "  {} findings  ·  ", summary.total_findings)?;
        if high > 0 {
            write!(out, "HIGH {high}  ")?;
        }
        if med > 0 {
            write!(out, "MED {med}  ")?;
        }
        if low > 0 {
            write!(out, "LOW {low}  ")?;
        }
        writeln!(out, "·  AI likelihood {pct:.0}%")?;
    }
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

    use super::{
        Ansi, build_summary, category_label, severity_label, write_github_actions_report,
        write_json_report, write_text_report,
    };
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
        write_text_report(&mut out, &[], &summary, false).expect("write");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains("No findings."));
    }

    #[test]
    fn empty_findings_with_color_uses_ansi_path() {
        // Covers lines 136-137: writeln! inside if use_color { ... } when findings empty.
        let summary = build_summary(&[]);
        let mut out = Vec::new();
        write_text_report(&mut out, &[], &summary, true).expect("write");
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
        write_text_report(&mut out, &findings, &summary, false).expect("write");
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
        assert!(
            text.contains("::error file=src/main.rs"),
            "expected ::error annotation"
        );
        assert!(
            text.contains("::notice title=papertowel summary::"),
            "expected summary notice"
        );
    }

    #[test]
    fn gha_report_escapes_percent_in_description() {
        let mut f = make_finding(Severity::Medium, "src/lib.rs");
        f.description = "100% AI-generated".to_owned();
        let summary = build_summary(&[f.clone()]);
        let mut out = Vec::new();
        write_github_actions_report(&mut out, &[f], &summary).expect("write");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains("100%25 AI-generated"),
            "percent must be escaped"
        );
    }

    #[test]
    fn gha_report_escapes_cr_and_lf() {
        let mut f = make_finding(Severity::High, "src/lib.rs");
        f.description = "line one\r\nline two".to_owned();
        let summary = build_summary(&[f.clone()]);
        let mut out = Vec::new();
        write_github_actions_report(&mut out, &[f], &summary).expect("write");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains("%0D%0A"), "CR+LF must be escaped");
    }

    #[test]
    fn text_report_with_color_enabled_runs_without_error() {
        let f = make_finding(Severity::High, ".");
        let summary = build_summary(&[f.clone()]);
        let mut out = Vec::new();
        write_text_report(&mut out, &[f], &summary, true).expect("write");
        let text = String::from_utf8(out).expect("utf8");
        assert!(!text.is_empty());
    }

    #[test]
    fn text_report_repo_root_displays_as_friendly_label() {
        let f = make_finding(Severity::Low, ".");
        let summary = build_summary(&[f.clone()]);
        let mut out = Vec::new();
        write_text_report(&mut out, &[f], &summary, false).expect("write");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains("(repo root)"),
            "path '.' should be displayed as (repo root)"
        );
    }

    #[test]
    fn category_label_covers_all_variants() {
        use crate::detection::finding::FindingCategory;
        assert_eq!(
            category_label(FindingCategory::CommitPattern),
            "commit_pattern"
        );
        assert_eq!(
            category_label(FindingCategory::PromptLeakage),
            "prompt_leakage"
        );
        assert_eq!(category_label(FindingCategory::TestPattern), "test_pattern");
        assert_eq!(
            category_label(FindingCategory::IdiomMismatch),
            "idiom_mismatch"
        );
        assert_eq!(
            category_label(FindingCategory::NameCredibility),
            "name_credibility"
        );
        assert_eq!(category_label(FindingCategory::Promotion), "promotion");
        assert_eq!(category_label(FindingCategory::Maintenance), "maintenance");
        assert_eq!(category_label(FindingCategory::Workflow), "workflow");
        assert_eq!(severity_label(Severity::High), "HIGH");
        assert_eq!(severity_label(Severity::Medium), "MED");
        assert_eq!(severity_label(Severity::Low), "LOW");
    }

    #[test]
    fn text_report_with_medium_and_low_color_covers_badge_paths() {
        // Covers Severity::Medium (line 97) and Severity::Low (line 98) color paths.
        let med = make_finding(Severity::Medium, "src/lib.rs");
        let low = make_finding(Severity::Low, "src/util.rs");
        let findings = vec![med, low];
        let summary = build_summary(&findings);
        let mut out = Vec::new();
        write_text_report(&mut out, &findings, &summary, true).expect("write");
        let text = String::from_utf8(out).expect("utf8");
        assert!(!text.is_empty());
    }

    #[test]
    fn text_report_with_line_range_and_suggestion_covers_location_paths() {
        // Covers lines 181-184 (line_range) and 188 (suggestion).
        let mut f = make_finding(Severity::High, "src/main.rs");
        f.line_range = Some(crate::detection::finding::LineRange::new(10, 20).expect("range"));
        f.suggestion = Some("remove this".to_owned());
        let findings = vec![f];
        let summary = build_summary(&findings);
        let mut out = Vec::new();
        write_text_report(&mut out, &findings, &summary, false).expect("write");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains("at src/main.rs:10"), "should show file:line");
        assert!(text.contains("remove this"), "should show suggestion");
    }

    #[test]
    fn gha_report_with_line_range_emits_line_annotation() {
        // Covers lines 307-308: GHA report with a finding that has a line_range.
        let mut f = make_finding(Severity::High, "src/main.rs");
        f.line_range = Some(crate::detection::finding::LineRange::new(5, 5).expect("range"));
        let findings = vec![f];
        let summary = build_summary(&findings);
        let mut out = Vec::new();
        write_github_actions_report(&mut out, &findings, &summary).expect("write");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains("line=5"),
            "GHA annotation should include line number"
        );
    }

    #[test]
    fn text_summary_with_color_and_med_low_findings() {
        // Covers lines 224-225 (MED color) and 233-234 (LOW color) in write_text_summary.
        // Also covers ai_prob_color YELLOW (line 113) and GREEN (line 115) paths.
        let high = make_finding(Severity::High, ".");
        let med = make_finding(Severity::Medium, ".");
        let low = make_finding(Severity::Low, ".");
        let findings = vec![high, med, low];
        let summary = build_summary(&findings);
        let mut out = Vec::new();
        write_text_report(&mut out, &findings, &summary, true).expect("write");
        let text = String::from_utf8(out).expect("utf8");
        assert!(!text.is_empty());
    }

    #[test]
    fn ai_prob_color_yellow_and_green_paths() {
        // Covers line 113 (YELLOW: prob >= 0.50 && prob < 0.75) and line 115 (GREEN).
        let a = Ansi { use_color: true };
        // YELLOW path: prob in [0.50, 0.75)
        assert_eq!(a.ai_prob_color(0.60), Ansi::YELLOW);
        // GREEN path: prob < 0.50
        assert_eq!(a.ai_prob_color(0.30), Ansi::GREEN);
    }

    #[test]
    fn category_label_covers_comment_structure_readme_metadata() {
        // Covers lines 350-353: Comment, Structure, Readme, Metadata variants.
        assert_eq!(category_label(FindingCategory::Comment), "comment");
        assert_eq!(category_label(FindingCategory::Structure), "structure");
        assert_eq!(category_label(FindingCategory::Readme), "readme");
        assert_eq!(category_label(FindingCategory::Metadata), "metadata");
    }
}
