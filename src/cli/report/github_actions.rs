use std::io::{self, Write};

use crate::detection::finding::Finding;

use super::ScanSummary;
use super::helpers::category_label;

pub(super) fn write_github_actions_report(
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
        "::notice title=papertowel summary::{total} finding(s) — AI probability {ai_pct:.0}%",
        total = summary.total_findings,
    )?;

    Ok(())
}

/// Escape payload data for GitHub Actions workflow command syntax
/// command (`::command key=value::data`).
fn escape_gha_data(s: &str) -> String {
    // GitHub Actions command data escaping: percent, carriage-return, newline.
    s.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}
