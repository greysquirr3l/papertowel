use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::domain::errors::PapertowelError;

const CORALINE_DIR: &str = ".coraline";
const CORALINE_FILE_LIST_CANDIDATES: [&str; 2] = ["files.list", "files.txt"];
const SOURCE_EXTENSIONS: [&str; 10] = [
    "rs", "go", "py", "ts", "tsx", "js", "jsx", "cs", "zig", "md",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoAnalysisBackend {
    CoralineIndex,
    FilesystemScan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendSelection {
    pub backend: RepoAnalysisBackend,
    pub coraline_dir: Option<PathBuf>,
}

#[must_use]
pub fn resolve_backend(repo_root: impl AsRef<Path>) -> BackendSelection {
    let coraline_dir = repo_root.as_ref().join(CORALINE_DIR);
    if coraline_dir.is_dir() {
        BackendSelection {
            backend: RepoAnalysisBackend::CoralineIndex,
            coraline_dir: Some(coraline_dir),
        }
    } else {
        BackendSelection {
            backend: RepoAnalysisBackend::FilesystemScan,
            coraline_dir: None,
        }
    }
}

pub fn collect_candidate_files(
    repo_root: impl AsRef<Path>,
) -> Result<Vec<PathBuf>, PapertowelError> {
    let repo_root = repo_root.as_ref();
    let selection = resolve_backend(repo_root);

    if let Some(coraline_dir) = selection.coraline_dir {
        let from_index = read_coraline_file_list(repo_root, &coraline_dir)?;
        if !from_index.is_empty() {
            return Ok(from_index);
        }
    }

    scan_repo_filesystem(repo_root)
}

fn read_coraline_file_list(
    repo_root: &Path,
    coraline_dir: &Path,
) -> Result<Vec<PathBuf>, PapertowelError> {
    for manifest_name in CORALINE_FILE_LIST_CANDIDATES {
        let manifest_path = coraline_dir.join(manifest_name);
        if !manifest_path.exists() {
            continue;
        }

        let content = fs::read_to_string(&manifest_path)
            .map_err(|error| PapertowelError::io_with_path(&manifest_path, error))?;

        let mut files = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            let candidate = repo_root.join(trimmed);
            if candidate.is_file() && has_source_extension(&candidate) {
                files.push(candidate);
            }
        }

        return Ok(files);
    }

    Ok(Vec::new())
}

fn scan_repo_filesystem(repo_root: &Path) -> Result<Vec<PathBuf>, PapertowelError> {
    let mut files = Vec::new();

    for entry in WalkDir::new(repo_root) {
        let entry = entry.map_err(|error| {
            let path = error
                .path()
                .map_or_else(|| repo_root.to_path_buf(), Path::to_path_buf);
            let io_error = std::io::Error::other(error.to_string());
            PapertowelError::io_with_path(path, io_error)
        })?;

        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        if is_ignored_dir(path) {
            continue;
        }

        if has_source_extension(path) {
            files.push(path.to_path_buf());
        }
    }

    Ok(files)
}

fn is_ignored_dir(path: &Path) -> bool {
    path.components().any(|component| {
        let name = component.as_os_str();
        name == OsStr::new(".git")
            || name == OsStr::new(CORALINE_DIR)
            || name == OsStr::new("target")
    })
}

fn has_source_extension(path: &Path) -> bool {
    let extension = path.extension().and_then(OsStr::to_str);
    extension.is_some_and(|ext| SOURCE_EXTENSIONS.contains(&ext))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::{RepoAnalysisBackend, collect_candidate_files, resolve_backend};

    #[test]
    fn resolve_backend_prefers_coraline_when_present() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = TempDir::new()?;

        let coraline_dir = tmp.path().join(".coraline");
        fs::create_dir_all(&coraline_dir)?;

        let selection = resolve_backend(tmp.path());
        assert_eq!(selection.backend, RepoAnalysisBackend::CoralineIndex);
        Ok(())
    }

    #[test]
    fn collect_candidate_files_falls_back_to_filesystem_scan()
    -> Result<(), Box<dyn std::error::Error>> {
        let tmp = TempDir::new()?;

        fs::write(tmp.path().join("main.rs"), "fn main() {}\n")?;

        let files = collect_candidate_files(tmp.path())?;
        assert_eq!(files.len(), 1);
        let first = files.first().ok_or("expected one file")?;
        assert!(first.ends_with("main.rs"));
        Ok(())
    }

    #[test]
    fn collect_candidate_files_uses_coraline_manifest_when_available()
    -> Result<(), Box<dyn std::error::Error>> {
        let tmp = TempDir::new()?;

        let src_path = tmp.path().join("src");
        fs::create_dir_all(&src_path)?;
        fs::write(src_path.join("lib.rs"), "pub fn x() {}\n")?;

        let coraline_dir = tmp.path().join(".coraline");
        fs::create_dir_all(&coraline_dir)?;

        let manifest = "src/lib.rs\n# comment\n\n";
        fs::write(coraline_dir.join("files.list"), manifest)?;

        let files = collect_candidate_files(tmp.path())?;
        assert_eq!(files.len(), 1);
        let first = files.first().ok_or("expected one file")?;
        assert!(first.ends_with("lib.rs"));
        Ok(())
    }
}
