use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::domain::errors::PapertowelError;

const CORALINE_DIR: &str = ".coraline";
const CORALINE_FILE_LIST_CANDIDATES: [&str; 2] = ["files.list", "files.txt"];
const SOURCE_EXTENSIONS: [&str; 10] = ["rs", "go", "py", "ts", "tsx", "js", "jsx", "cs", "zig", "md"];

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

pub fn collect_candidate_files(repo_root: impl AsRef<Path>) -> Result<Vec<PathBuf>, PapertowelError> {
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
        name == OsStr::new(".git") || name == OsStr::new(CORALINE_DIR) || name == OsStr::new("target")
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

    use super::{collect_candidate_files, resolve_backend, RepoAnalysisBackend};

    #[test]
    fn resolve_backend_prefers_coraline_when_present() {
        let tmp = TempDir::new();
        assert!(tmp.is_ok());
        let tmp = match tmp {
            Ok(tmp) => tmp,
            Err(error) => panic!("failed to create tempdir: {error}"),
        };

        let coraline_dir = tmp.path().join(".coraline");
        let created = fs::create_dir_all(&coraline_dir);
        assert!(created.is_ok());

        let selection = resolve_backend(tmp.path());
        assert_eq!(selection.backend, RepoAnalysisBackend::CoralineIndex);
    }

    #[test]
    fn collect_candidate_files_falls_back_to_filesystem_scan() {
        let tmp = TempDir::new();
        assert!(tmp.is_ok());
        let tmp = match tmp {
            Ok(tmp) => tmp,
            Err(error) => panic!("failed to create tempdir: {error}"),
        };

        let write_result = fs::write(tmp.path().join("main.rs"), "fn main() {}\n");
        assert!(write_result.is_ok());

        let files = collect_candidate_files(tmp.path());
        assert!(files.is_ok());
        let files = match files {
            Ok(files) => files,
            Err(error) => panic!("unexpected collection error: {error}"),
        };

        assert_eq!(files.len(), 1);
        let first = files.first();
        assert!(first.is_some());
        let first = match first {
            Some(first) => first,
            None => panic!("expected one file"),
        };
        assert!(first.ends_with("main.rs"));
    }

    #[test]
    fn collect_candidate_files_uses_coraline_manifest_when_available() {
        let tmp = TempDir::new();
        assert!(tmp.is_ok());
        let tmp = match tmp {
            Ok(tmp) => tmp,
            Err(error) => panic!("failed to create tempdir: {error}"),
        };

        let src_path = tmp.path().join("src");
        let created_src = fs::create_dir_all(&src_path);
        assert!(created_src.is_ok());
        let write_source = fs::write(src_path.join("lib.rs"), "pub fn x() {}\n");
        assert!(write_source.is_ok());

        let coraline_dir = tmp.path().join(".coraline");
        let created_coraline = fs::create_dir_all(&coraline_dir);
        assert!(created_coraline.is_ok());

        let manifest = "src/lib.rs\n# comment\n\n";
        let write_manifest = fs::write(coraline_dir.join("files.list"), manifest);
        assert!(write_manifest.is_ok());

        let files = collect_candidate_files(tmp.path());
        assert!(files.is_ok());
        let files = match files {
            Ok(files) => files,
            Err(error) => panic!("unexpected collection error: {error}"),
        };

        assert_eq!(files.len(), 1);
        let first = files.first();
        assert!(first.is_some());
        let first = match first {
            Some(first) => first,
            None => panic!("expected one file"),
        };
        assert!(first.ends_with("lib.rs"));
    }
}