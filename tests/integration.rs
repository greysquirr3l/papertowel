use std::path::PathBuf;

use assert_cmd::Command;
use predicates::prelude::*;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

// ── clean_repo ────────────────────────────────────────────────────────────────

#[test]
fn scan_clean_repo_exits_zero() {
    let mut cmd = Command::cargo_bin("papertowel").unwrap();
    cmd.arg("scan").arg(fixture("clean_repo"));
    cmd.assert().success();
}

#[test]
fn scan_clean_repo_fail_on_medium_exits_zero() {
    // Plain human-written code should not reach Medium severity.
    let mut cmd = Command::cargo_bin("papertowel").unwrap();
    cmd.arg("scan")
        .arg(fixture("clean_repo"))
        .args(["--fail-on", "medium"]);
    cmd.assert().success();
}

// ── slop_repo ─────────────────────────────────────────────────────────────────

#[test]
fn scan_slop_repo_exits_zero_without_fail_on() {
    // scan should always exit 0 when no --fail-on gate is set.
    let mut cmd = Command::cargo_bin("papertowel").unwrap();
    cmd.arg("scan").arg(fixture("slop_repo"));
    cmd.assert().success();
}

#[test]
fn scan_slop_repo_produces_output() {
    // Slop-heavy source should produce at least some finding output.
    let mut cmd = Command::cargo_bin("papertowel").unwrap();
    cmd.arg("scan").arg(fixture("slop_repo"));
    cmd.assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

#[test]
fn scan_slop_repo_fail_on_medium_exits_nonzero() {
    // Clustered slop vocabulary should reach at least Medium severity,
    // causing the gate to exit nonzero.
    let mut cmd = Command::cargo_bin("papertowel").unwrap();
    cmd.arg("scan")
        .arg(fixture("slop_repo"))
        .args(["--fail-on", "medium"]);
    cmd.assert().failure();
}

#[test]
fn scan_slop_repo_json_is_valid() {
    // JSON mode must produce parseable output.
    let mut cmd = Command::cargo_bin("papertowel").unwrap();
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
    let mut cmd = Command::cargo_bin("papertowel").unwrap();
    cmd.arg("scan").arg(fixture("template_repo"));
    cmd.assert().success();
}

#[test]
fn scan_template_repo_produces_readme_findings() {
    // The template README triggers emoji-header and badge-wall patterns.
    let mut cmd = Command::cargo_bin("papertowel").unwrap();
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
