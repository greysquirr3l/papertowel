use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PapertowelError {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("detection error: {0}")]
    Detection(String),
    #[error("git error: {0}")]
    Git(String),
    #[error("i/o error at {path}: {message}")]
    Io { path: PathBuf, message: String },
}