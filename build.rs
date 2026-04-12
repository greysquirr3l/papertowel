//! Build script — embeds a build identifier at compile time.
//!
//! Sets the `PAPERTOWEL_GIT_SHA` env var, available at runtime via
//! `env!("PAPERTOWEL_GIT_SHA")`.

use std::env;
use std::process::Command;

fn detect_build_id() -> String {
    let git_sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned());

    if let Some(sha) = git_sha {
        return sha;
    }

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    if manifest_dir.contains("/.cargo/registry/src/") {
        return "crates.io".to_owned();
    }

    "source".to_owned()
}

fn main() {
    // Rerun when HEAD or any branch ref changes.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads/");

    let sha = detect_build_id();

    println!("cargo:rustc-env=PAPERTOWEL_GIT_SHA={sha}");
}
