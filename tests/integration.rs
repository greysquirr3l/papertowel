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
