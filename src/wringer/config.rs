use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::errors::PapertowelError;
use crate::wringer::worktree::WorktreeSpec;

/// Persistent wringer state stored in `.papertowel/wringer.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WringerConfig {
    pub branch: String,
    pub worktree_path: PathBuf,
    pub worktree_name: String,
}

impl WringerConfig {
    pub fn to_spec(&self) -> WorktreeSpec {
        WorktreeSpec {
            name: self.worktree_name.clone(),
            branch: self.branch.clone(),
            path: self.worktree_path.clone(),
        }
    }
}

fn config_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".papertowel").join("wringer.toml")
}

pub fn save_wringer_config(
    repo_root: &Path,
    config: &WringerConfig,
) -> Result<(), PapertowelError> {
    let path = config_path(repo_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| PapertowelError::io_with_path(parent, error))?;
    }
    let content =
        toml::to_string_pretty(config).map_err(|error| PapertowelError::Config(error.to_string()))?;
    fs::write(&path, content).map_err(|error| PapertowelError::io_with_path(&path, error))
}

pub fn load_wringer_config(repo_root: &Path) -> Result<WringerConfig, PapertowelError> {
    let path = config_path(repo_root);
    let content =
        fs::read_to_string(&path).map_err(|error| PapertowelError::io_with_path(&path, error))?;
    toml::from_str(&content).map_err(|error| PapertowelError::Config(error.to_string()))
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::{WringerConfig, load_wringer_config, save_wringer_config};

    #[test]
    fn config_roundtrips_toml() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = TempDir::new()?;
        let config = WringerConfig {
            branch: String::from("public"),
            worktree_name: String::from("public"),
            worktree_path: tmp.path().join("public-worktree"),
        };

        save_wringer_config(tmp.path(), &config)?;
        let loaded = load_wringer_config(tmp.path())?;

        assert_eq!(loaded.branch, config.branch);
        assert_eq!(loaded.worktree_name, config.worktree_name);
        assert_eq!(loaded.worktree_path, config.worktree_path);
        Ok(())
    }

    #[test]
    fn to_spec_produces_matching_worktree_spec() {
        let path = std::path::PathBuf::from("/tmp/public-worktree");
        let config = WringerConfig {
            branch: String::from("public"),
            worktree_name: String::from("public"),
            worktree_path: path.clone(),
        };
        let spec = config.to_spec();
        assert_eq!(spec.branch, "public");
        assert_eq!(spec.name, "public");
        assert_eq!(spec.path, path);
    }
}
