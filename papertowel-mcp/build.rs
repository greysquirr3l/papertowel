//! Build script — embeds the current git SHA at compile time.
//!
//! Sets the `PAPERTOWEL_GIT_SHA` env var, available at runtime via
//! `env!("PAPERTOWEL_GIT_SHA")`.

use std::process::Command;

fn main() {
    // Rerun when HEAD or any branch ref changes.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads/");

    let sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map_or_else(|| "unknown".to_owned(), |s| s.trim().to_owned());

    println!("cargo:rustc-env=PAPERTOWEL_GIT_SHA={sha}");
}
