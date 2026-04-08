use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PapertowelError {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("validation error: {0}")]
    Validation(String),
    #[error("detection error: {0}")]
    Detection(String),
    #[error("git operation failed: {0}")]
    Git(#[from] git2::Error),
    #[error("i/o error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse TOML: {0}")]
    TomlDeserialize(#[from] toml::de::Error),
    #[error("failed to serialize TOML: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
}

impl PapertowelError {
    pub fn io_with_path(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PapertowelError;

    #[test]
    fn io_error_with_path_mentions_path() {
        let error = PapertowelError::io_with_path(
            "src/main.rs",
            std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
        );

        let rendered = error.to_string();
        assert!(rendered.contains("src/main.rs"));
    }
}