use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Result, anyhow};
use clap::Args;

const MCP_PROTOCOL_VERSION: &str = "2025-11-25";

#[derive(Debug, Args, Default)]
pub struct ServeArgs {}

#[derive(Debug, Args, Default)]
pub struct ToolsArgs {}

#[derive(Debug, Args, Default)]
pub struct VersionArgs {}

pub fn handle_serve(_args: &ServeArgs) -> Result<()> {
    let executable = resolve_mcp_executable()?;

    let mut command = Command::new(&executable);
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let status = command.status().map_err(|error| {
        anyhow!(
            "failed to start MCP server executable {}: {error}",
            executable.display()
        )
    })?;

    if status.success() {
        Ok(())
    } else {
        Err(anyhow!(
            "MCP server process {} exited with status {status}",
            executable.display()
        ))
    }
}

pub fn handle_tools(_args: &ToolsArgs) {
    println!("papertowel_scan");
    println!("papertowel_scrub");
    println!("papertowel_grade");
}

pub fn handle_version(_args: &VersionArgs) {
    println!("papertowel-mcp protocol {MCP_PROTOCOL_VERSION}");
    println!(
        "server {} ({})",
        env!("CARGO_PKG_VERSION"),
        env!("PAPERTOWEL_GIT_SHA")
    );
}

fn resolve_mcp_executable() -> Result<OsString> {
    if let Some(path) = env::var_os("PAPERTOWEL_MCP_PATH") {
        let configured = PathBuf::from(path);
        if configured.exists() {
            return Ok(configured.into_os_string());
        }

        return Err(anyhow!(
            "PAPERTOWEL_MCP_PATH points to {}, but that file was not found",
            configured.display()
        ));
    }

    let binary_name = format!("papertowel-mcp{}", env::consts::EXE_SUFFIX);
    for candidate in candidate_paths(&binary_name) {
        if candidate.exists() {
            return Ok(candidate.into_os_string());
        }
    }

    Err(anyhow!(
        "could not find {binary_name} next to the papertowel binary. Install papertowel-mcp or set PAPERTOWEL_MCP_PATH to the executable location"
    ))
}

fn candidate_paths(binary_name: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(current_exe) = env::current_exe()
        && let Some(parent) = current_exe.parent()
    {
        candidates.push(parent.join(binary_name));
    }

    candidates
}
