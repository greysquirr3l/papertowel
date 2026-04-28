use std::io::{self, Write};

use crate::detection::finding::{Finding, Severity};

use super::ScanSummary;
use super::helpers::category_label;

pub(super) fn write_sarif_report(
    out: &mut impl Write,
    findings: &[Finding],
    summary: &ScanSummary,
) -> io::Result<()> {
    let rules = build_sarif_rules(findings);
    let results = build_sarif_results(findings, &rules);

    let tool = serde_json::json!({
        "driver": {
            "name": "papertowel",
            "version": env!("CARGO_PKG_VERSION"),
            "informationUri": "https://github.com/greysquirr3l/papertowel",
            "rules": rules.iter().map(|(_, rule)| rule.clone()).collect::<Vec<_>>(),
        }
    });

    let run = serde_json::json!({
        "tool": tool,
        "results": results,
        "properties": {
            "papertowel": {
                "totalFindings": summary.total_findings,
                "aiProbability": summary.ai_probability,
                "bySeverity": summary.by_severity,
                "byCategory": summary.by_category,
            }
        }
    });

    let sarif = serde_json::json!({
        "$schema": "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/sarif-2.1/schema/sarif-schema-2.1.0.json",
        "version": "2.1.0",
        "runs": [run],
    });

    let json = serde_json::to_string_pretty(&sarif).map_err(io::Error::other)?;
    writeln!(out, "{json}")
}

/// Build a deduplicated rule table keyed by `(category, id)`.
fn build_sarif_rules(findings: &[Finding]) -> Vec<(String, serde_json::Value)> {
    let mut seen = std::collections::HashSet::new();
    let mut rules = Vec::new();

    for f in findings {
        let rule_id = sarif_rule_id(f);
        if seen.insert(rule_id.clone()) {
            let mut rule = serde_json::json!({
                "id": rule_id,
                "shortDescription": { "text": format!("{} detector", category_label(f.category)) },
            });
            if let Some(ref suggestion) = f.suggestion {
                rule.as_object_mut().and_then(|m| {
                    m.insert("helpUri".to_owned(), serde_json::Value::Null);
                    m.insert("help".to_owned(), serde_json::json!({ "text": suggestion }))
                });
            }
            rules.push((rule_id, rule));
        }
    }
    rules
}

fn build_sarif_results(
    findings: &[Finding],
    rules: &[(String, serde_json::Value)],
) -> Vec<serde_json::Value> {
    findings
        .iter()
        .map(|f| {
            let rule_id = sarif_rule_id(f);
            let rule_index = rules.iter().position(|(id, _)| *id == rule_id).unwrap_or(0);

            let mut location = serde_json::json!({
                "physicalLocation": {
                    "artifactLocation": {
                        "uri": f.file_path.to_string_lossy(),
                        "uriBaseId": "%SRCROOT%",
                    }
                }
            });

            if let Some(range) = f.line_range {
                location
                    .as_object_mut()
                    .and_then(|m| m.get_mut("physicalLocation"))
                    .and_then(|pl| pl.as_object_mut())
                    .and_then(|pl| {
                        pl.insert(
                            "region".to_owned(),
                            serde_json::json!({
                                "startLine": range.start,
                                "endLine": range.end,
                            }),
                        )
                    });
            }

            let mut result = serde_json::json!({
                "ruleId": rule_id,
                "ruleIndex": rule_index,
                "level": sarif_level(f.severity),
                "message": { "text": f.description },
                "locations": [location],
                "properties": {
                    "confidenceScore": f.confidence_score,
                    "autoFixable": f.auto_fixable,
                }
            });

            if let Some(ref suggestion) = f.suggestion {
                result.as_object_mut().and_then(|m| {
                    m.insert(
                        "fixes".to_owned(),
                        serde_json::json!([{
                            "description": { "text": suggestion },
                        }]),
                    )
                });
            }

            result
        })
        .collect()
}

fn sarif_rule_id(f: &Finding) -> String {
    format!("papertowel/{}/{}", category_label(f.category), f.id)
}

const fn sarif_level(s: Severity) -> &'static str {
    match s {
        Severity::High => "error",
        Severity::Medium => "warning",
        Severity::Low => "note",
    }
}
