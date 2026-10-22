#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::indexing_slicing, reason = "test assertions")]

use std::path::PathBuf;

use assert_cmd::Command;
use predicates::prelude::*;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// Build a papertowel [`Command`] with `CI` removed from the environment so
/// the binary runs in interactive (non-CI) mode regardless of where the test
/// suite itself is executed. Without this, GitHub Actions sets `CI=true` which
/// auto-enables `--fail-on medium` and switches output to GHA annotation
/// format, breaking tests that expect plain-text stdout and exit 0.
fn cmd() -> Command {
    let mut c = Command::cargo_bin("papertowel").unwrap();
    c.env_remove("CI");
    c
}

// ── clean_repo ────────────────────────────────────────────────────────────────

#[test]
fn scan_clean_repo_exits_zero() {
    let mut cmd = cmd();
    cmd.arg("scan").arg(fixture("clean_repo"));
    cmd.assert().success();
}

#[test]
fn scan_clean_repo_fail_on_medium_exits_zero() {
    // Plain human-written code should not reach Medium severity.
    let mut cmd = cmd();
    cmd.arg("scan")
        .arg(fixture("clean_repo"))
        .args(["--fail-on", "medium"]);
    cmd.assert().success();
}

// ── slop_repo ─────────────────────────────────────────────────────────────────

#[test]
fn scan_slop_repo_exits_zero_without_fail_on() {
    // scan should always exit 0 when no --fail-on gate is set.
    let mut cmd = cmd();
    cmd.arg("scan").arg(fixture("slop_repo"));
    cmd.assert().success();
}

#[test]
fn scan_slop_repo_produces_output() {
    // Slop-heavy source should produce at least some finding output.
    let mut cmd = cmd();
    cmd.arg("scan").arg(fixture("slop_repo"));
    cmd.assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

#[test]
fn scan_slop_repo_fail_on_medium_exits_nonzero() {
    // Clustered slop vocabulary should reach at least Medium severity,
    // causing the gate to exit nonzero.
    let mut cmd = cmd();
    cmd.arg("scan")
        .arg(fixture("slop_repo"))
        .args(["--fail-on", "medium"]);
    cmd.assert().failure();
}

#[test]
fn scan_slop_repo_json_is_valid() {
    // JSON mode must produce parseable output.
    let mut cmd = cmd();
    cmd.arg("scan")
        .arg(fixture("slop_repo"))
        .args(["--format", "json"]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let text = String::from_utf8(output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    assert!(parsed.is_object());
    assert!(parsed["findings"].is_array());
}

// ── template_repo ─────────────────────────────────────────────────────────────

#[test]
fn scan_template_repo_exits_zero_without_fail_on() {
    let mut cmd = cmd();
    cmd.arg("scan").arg(fixture("template_repo"));
    cmd.assert().success();
}

#[test]
fn scan_template_repo_produces_readme_findings() {
    // The template README triggers emoji-header and badge-wall patterns.
    let mut cmd = cmd();
    cmd.arg("scan")
        .arg(fixture("template_repo"))
        .args(["--format", "json"]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let text = String::from_utf8(output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    let findings = parsed["findings"].as_array().expect("findings array");
    assert!(
        !findings.is_empty(),
        "expected findings from template README, got none"
    );
}

// ── rule overrides ────────────────────────────────────────────────────────────

/// Create a throwaway project containing a single file that uses the word
/// "harness" (a hardcoded slop term) and return its root.
fn harness_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("src").join("main.rs"),
        "fn main() {\n    let harness = TestHarness::new();\n    harness.run_all();\n}\n",
    )
    .unwrap();
    dir
}

/// Extract the `id` fields from a JSON scan report.
fn finding_ids(json: &str) -> Vec<String> {
    let parsed: serde_json::Value = serde_json::from_str(json).expect("valid JSON");
    parsed["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .filter_map(|f| f["id"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn exclude_terms_suppresses_recipe_backed_slop_word() {
    let dir = harness_project();
    std::fs::write(
        dir.path().join(".papertowel.toml"),
        "[detectors.lexical]\nexclude_terms = [\"harness\"]\n",
    )
    .unwrap();

    let mut cmd = cmd();
    cmd.arg("scan").arg(dir.path()).args(["--format", "json"]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let text = String::from_utf8(output).unwrap();

    let ids = finding_ids(&text);
    assert!(
        !ids.iter().any(|id| id == "slop-vocabulary:word:harness"),
        "expected the 'harness' rule to be suppressed, got {ids:?}"
    );
}

#[test]
fn harness_rule_is_reported_without_a_config_file() {
    // Sanity check: the same project without a config does report the rule.
    let dir = harness_project();

    let mut cmd = cmd();
    cmd.arg("scan").arg(dir.path()).args(["--format", "json"]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let text = String::from_utf8(output).unwrap();

    let ids = finding_ids(&text);
    assert!(
        ids.iter().any(|id| id == "slop-vocabulary:word:harness"),
        "expected the 'harness' rule to be reported, got {ids:?}"
    );
}

#[test]
fn rules_disable_suppresses_recipe_backed_slop_word() {
    let dir = harness_project();
    std::fs::write(
        dir.path().join(".papertowel.toml"),
        "[rules]\ndisable = [\"slop-vocabulary:word:harness\"]\n",
    )
    .unwrap();

    let mut cmd = cmd();
    cmd.arg("scan").arg(dir.path()).args(["--format", "json"]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let text = String::from_utf8(output).unwrap();

    let ids = finding_ids(&text);
    assert!(
        !ids.iter().any(|id| id == "slop-vocabulary:word:harness"),
        "expected the 'harness' rule to be disabled, got {ids:?}"
    );
}

#[test]
fn rules_severity_overrides_recipe_backed_slop_word() {
    let dir = harness_project();
    std::fs::write(
        dir.path().join(".papertowel.toml"),
        "[rules.severity]\n\"slop-vocabulary:word:harness\" = \"high\"\n",
    )
    .unwrap();

    let mut cmd = cmd();
    cmd.arg("scan").arg(dir.path()).args(["--format", "json"]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let text = String::from_utf8(output).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    let findings = parsed["findings"].as_array().expect("findings array");
    let harness = findings
        .iter()
        .find(|f| f["id"] == "slop-vocabulary:word:harness")
        .expect("harness finding");
    assert_eq!(harness["severity"], "High");
}

#[test]
fn rules_disable_suppresses_builtin_detector_rule() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("src").join("handlers.py"),
        "def run(x):\n    return eval(x)\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join(".papertowel.toml"),
        "[rules]\ndisable = [\"SEC002\"]\n",
    )
    .unwrap();

    let mut cmd = cmd();
    cmd.arg("scan").arg(dir.path()).args(["--format", "json"]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let text = String::from_utf8(output).unwrap();

    let ids = finding_ids(&text);
    assert!(
        !ids.iter().any(|id| id == "SEC002"),
        "expected SEC002 to be disabled, got {ids:?}"
    );
}

#[test]
fn exclude_terms_prevents_scrub_from_rewriting_the_term() {
    let dir = harness_project();
    std::fs::write(
        dir.path().join(".papertowel.toml"),
        "[detectors.lexical]\nexclude_terms = [\"harness\"]\n",
    )
    .unwrap();

    let mut cmd = cmd();
    cmd.arg("scrub").arg(dir.path()).arg("--dry-run");
    let output = cmd.assert().success().get_output().stdout.clone();
    let text = String::from_utf8(output).unwrap();

    assert!(
        !text.contains("recipe"),
        "expected no recipe replacements for an excluded term, got: {text}"
    );
}

#[test]
fn rules_disable_prevents_scrub_from_rewriting_the_term() {
    let dir = harness_project();
    std::fs::write(
        dir.path().join(".papertowel.toml"),
        "[rules]\ndisable = [\"slop-vocabulary:word:harness\"]\n",
    )
    .unwrap();

    let mut cmd = cmd();
    cmd.arg("scrub").arg(dir.path()).arg("--dry-run");
    let output = cmd.assert().success().get_output().stdout.clone();
    let text = String::from_utf8(output).unwrap();

    assert!(
        !text.contains("recipe"),
        "expected no recipe replacements for a disabled rule, got: {text}"
    );
}

#[test]
fn rules_disable_prevents_scrub_from_stripping_disabled_comments() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    // Repeated comment prefixes are what the comments scrubber strips.
    std::fs::write(
        dir.path().join("src").join("lib.rs"),
        "// Step one handles the input parsing\n\
         // Step one handles the output formatting\n\
         // Step one handles the error mapping\n\
         // Step one handles the retry logic\n\
         pub fn thing() {}\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join(".papertowel.toml"),
        "[rules]\ndisable = [\"comments.over_documented\"]\n",
    )
    .unwrap();

    let mut cmd = cmd();
    cmd.arg("scrub").arg(dir.path()).arg("--dry-run");
    let output = cmd.assert().success().get_output().stdout.clone();
    let text = String::from_utf8(output).unwrap();

    assert!(
        !text.contains("comment"),
        "expected no comment stripping for a disabled rule, got: {text}"
    );
}

#[test]
fn comments_scrubber_strips_comments_when_the_rule_is_enabled() {
    // Control case for the test above: the same file *is* scrubbed when the
    // rule is not disabled.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("src").join("lib.rs"),
        "// Step one handles the input parsing\n\
         // Step one handles the output formatting\n\
         // Step one handles the error mapping\n\
         // Step one handles the retry logic\n\
         pub fn thing() {}\n",
    )
    .unwrap();

    let mut cmd = cmd();
    cmd.arg("scrub").arg(dir.path()).arg("--dry-run");
    let output = cmd.assert().success().get_output().stdout.clone();
    let text = String::from_utf8(output).unwrap();

    assert!(
        text.contains("comment"),
        "expected comment stripping when the rule is enabled, got: {text}"
    );
}

// ── detector name validation ──────────────────────────────────────────────────

#[test]
fn scrub_rejects_unknown_detector_name() {
    // An unknown detector used to be a silent no-op: exit 0, "no changes
    // needed". That reads as success, which is the wrong signal for a
    // CI-gated tool.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src").join("main.rs"), "fn main() {}\n").unwrap();

    let mut cmd = cmd();
    cmd.arg("scrub")
        .arg(dir.path())
        .arg("--dry-run")
        .args(["--detectors", "totally-bogus"]);
    cmd.assert().failure();
}

#[test]
fn scrub_rejects_misspelled_detector_name() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src").join("main.rs"), "fn main() {}\n").unwrap();

    let mut cmd = cmd();
    cmd.arg("scrub")
        .arg(dir.path())
        .arg("--dry-run")
        .args(["--detectors", "recipie"]);
    cmd.assert().failure();
}

#[test]
fn scrub_error_lists_valid_detector_names() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src").join("main.rs"), "fn main() {}\n").unwrap();

    let mut cmd = cmd();
    cmd.arg("scrub")
        .arg(dir.path())
        .arg("--dry-run")
        .args(["--detectors", "totally-bogus"]);
    let output = cmd.assert().failure().get_output().stderr.clone();
    let text = String::from_utf8(output).unwrap();

    assert!(text.contains("totally-bogus"), "{text}");
    assert!(text.contains("recipe"), "{text}");
    assert!(text.contains("comments"), "{text}");
    assert!(text.contains("readme"), "{text}");
}

#[test]
fn scrub_accepts_every_valid_detector_name() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src").join("main.rs"), "fn main() {}\n").unwrap();

    for name in ["recipe", "lexical", "comments", "readme"] {
        let mut cmd = cmd();
        cmd.arg("scrub")
            .arg(dir.path())
            .arg("--dry-run")
            .args(["--detectors", name]);
        cmd.assert().success();
    }
}

// ── comment decoration patterns ──────────────────────────────────────────────

/// Write `contents` to `src/main.rs` in a fresh temp project and return the
/// scan finding ids.
fn scan_ids_for_source(contents: &str) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src").join("main.rs"), contents).unwrap();

    let mut cmd = cmd();
    cmd.arg("scan").arg(dir.path()).args(["--format", "json"]);
    let output = cmd.assert().success().get_output().stdout.clone();
    finding_ids(&String::from_utf8(output).unwrap())
}

#[test]
fn decorative_separator_comment_is_reported() {
    let ids = scan_ids_for_source("// ================\nfn main() {}\n");
    assert!(
        ids.iter()
            .any(|id| id == "comment-patterns:regex:decorative-separator-line"),
        "expected the separator rule to fire, got {ids:?}"
    );
}

#[test]
fn banner_comment_is_reported() {
    let ids = scan_ids_for_source("// ------------- WORKFLOW -------------\nfn main() {}\n");
    assert!(
        ids.iter()
            .any(|id| id == "comment-patterns:regex:decorative-separator-banner"),
        "expected the banner rule to fire, got {ids:?}"
    );
}

#[test]
fn block_end_marker_comment_is_reported() {
    let ids = scan_ids_for_source("fn main() {\n} // end if\n");
    assert!(
        ids.iter()
            .any(|id| id == "comment-patterns:regex:block-end-marker"),
        "expected the end-marker rule to fire, got {ids:?}"
    );
}

#[test]
fn decorative_emoji_comment_is_reported() {
    let ids = scan_ids_for_source("// ✅ Validation\nfn main() {}\n");
    assert!(
        ids.iter()
            .any(|id| id == "comment-patterns:regex:decorative-emoji"),
        "expected the emoji rule to fire, got {ids:?}"
    );
}

#[test]
fn vague_todo_comment_is_reported() {
    let ids = scan_ids_for_source("// TODO: Improve this\nfn main() {}\n");
    assert!(
        ids.iter()
            .any(|id| id == "comment-patterns:regex:vague-todo"),
        "expected the vague-todo rule to fire, got {ids:?}"
    );
}

#[test]
fn specific_todo_comment_is_not_reported() {
    // A TODO that names an actionable task is legitimate and must not fire.
    let ids = scan_ids_for_source("// TODO: fix the off-by-one in parse_len\nfn main() {}\n");
    assert!(
        !ids.iter()
            .any(|id| id == "comment-patterns:regex:vague-todo"),
        "expected the vague-todo rule to stay quiet, got {ids:?}"
    );
}

#[test]
fn empty_section_label_comment_is_reported() {
    let ids = scan_ids_for_source("// Main logic\nfn main() {}\n");
    assert!(
        ids.iter()
            .any(|id| id == "comment-patterns:regex:empty-section-label"),
        "expected the section-label rule to fire, got {ids:?}"
    );
}

#[test]
fn emoji_documentation_of_zwj_sequence_is_not_reported() {
    // An emoji that documents a codepoint is information, not decoration.
    let ids = scan_ids_for_source("// 🚀 (U+1F680) + ZWJ + 🔥 (U+1F525)\nfn main() {}\n");
    assert!(
        !ids.iter()
            .any(|id| id == "comment-patterns:regex:decorative-emoji"),
        "expected the emoji rule to stay quiet, got {ids:?}"
    );
}

#[test]
fn three_dash_section_divider_is_not_reported() {
    // `// ─── Section ───` is a common, legible divider convention.
    let ids = scan_ids_for_source("// ─── Domain types ───\nfn main() {}\n");
    assert!(
        !ids.iter().any(|id| id.contains("decorative-separator")),
        "expected the separator rules to stay quiet, got {ids:?}"
    );
}

#[test]
fn legitimate_comments_are_not_reported() {
    let ids = scan_ids_for_source(
        "// Note: retries happen only on 5xx\n\
         // See https://example.com/docs\n\
         // --- see below for details\n\
         // TODO: add tests for the retry path\n\
         fn main() {}\n",
    );
    assert!(
        !ids.iter().any(|id| id.starts_with("comment-patterns:")),
        "expected no comment-pattern findings, got {ids:?}"
    );
}

// ── prose patterns ───────────────────────────────────────────────────────────

#[test]
fn chatbot_closer_phrase_is_reported() {
    let ids = scan_ids_for_source(
        "//! Here is the summary.\n\
         //!\n\
         //! Let me know if you have any questions.\n\
         fn main() {}\n",
    );
    assert!(
        ids.iter()
            .any(|id| id.contains("Let-me-know-if-you-have-any-questions")),
        "expected the chatbot-closer phrase to fire, got {ids:?}"
    );
}

#[test]
fn persuasive_authority_trope_is_reported() {
    let ids = scan_ids_for_source(
        "//! At its core, the real question is whether it scales.\nfn main() {}\n",
    );
    assert!(
        ids.iter().any(|id| id.contains("The-real-question-is")),
        "expected the authority trope to fire, got {ids:?}"
    );
}

#[test]
fn ordinary_prose_is_not_reported() {
    let ids = scan_ids_for_source(
        "//! The parser reads the header and returns the record.\n\
         //! It fails when the length field is missing.\n\
         fn main() {}\n",
    );
    assert!(
        !ids.iter()
            .any(|id| id.starts_with("phrase-patterns:phrase:")),
        "expected no phrase findings, got {ids:?}"
    );
}

// ── dash patterns ────────────────────────────────────────────────────────────

#[test]
fn em_dash_is_reported() {
    let ids = scan_ids_for_source("//! The policy — announced without warning — applies.\n");
    assert!(
        ids.iter()
            .any(|id| id == "dash-patterns:regex:spaced-em-dash"),
        "expected the spaced em-dash rule to fire, got {ids:?}"
    );
}

#[test]
fn unspaced_em_dash_is_reported() {
    let ids = scan_ids_for_source("//! A word—joined—together.\n");
    assert!(
        ids.iter().any(|id| id == "dash-patterns:regex:em-dash"),
        "expected the em-dash rule to fire, got {ids:?}"
    );
}

#[test]
fn a_single_em_dash_produces_exactly_one_finding() {
    // The em-dash and spaced-em-dash rules are mutually exclusive, so one
    // dash must not be counted by both. Findings are reported per line per
    // rule, so a line with two spaced em dashes yields one finding.
    let ids = scan_ids_for_source("//! The policy — announced without warning — applies.\n");
    let dash_findings = ids
        .iter()
        .filter(|id| id.starts_with("dash-patterns:"))
        .collect::<Vec<_>>();
    assert_eq!(
        dash_findings.len(),
        1,
        "expected exactly one dash finding, got {dash_findings:?}"
    );
    assert_eq!(dash_findings[0], "dash-patterns:regex:spaced-em-dash");
}

#[test]
fn double_hyphen_dash_is_reported() {
    let ids = scan_ids_for_source("//! An address -- yet this mislabeling continues.\n");
    assert!(
        ids.iter()
            .any(|id| id == "dash-patterns:regex:double-hyphen"),
        "expected the double-hyphen rule to fire, got {ids:?}"
    );
}

#[test]
fn command_line_flags_are_not_dashes() {
    // `--verbose` is a flag, not a dash used as a connector.
    let ids = scan_ids_for_source("//! Run with --verbose for more output.\n");
    assert!(
        !ids.iter().any(|id| id.starts_with("dash-patterns:")),
        "expected no dash findings, got {ids:?}"
    );
}

#[test]
fn en_dash_in_a_range_is_not_reported() {
    // An en dash in a numeric range is correct typography, not a tell.
    let ids = scan_ids_for_source("//! Supported from 2020–2024 onwards.\n");
    assert!(
        !ids.iter().any(|id| id.starts_with("dash-patterns:")),
        "expected no dash findings, got {ids:?}"
    );
}

#[test]
fn markdown_horizontal_rule_is_not_a_dash() {
    // `---` is a horizontal rule / frontmatter delimiter, not a dash.
    let ids = scan_ids_for_source("//! # Title\n//!\n//! ---\n//!\n//! Body text.\n");
    assert!(
        !ids.iter().any(|id| id.starts_with("dash-patterns:")),
        "expected no dash findings, got {ids:?}"
    );
}
