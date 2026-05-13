use thiserror::Error;

#[derive(Debug, Error)]
pub enum DvcError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("hash mismatch: expected {expected}, found {actual}")]
    HashMismatch { expected: String, actual: String },

    #[error("manifest error: {0}")]
    Manifest(#[from] rusqlite::Error),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),
}

pub type Result<T> = std::result::Result<T, DvcError>;
