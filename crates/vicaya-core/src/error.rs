//! Error types for vicaya.

use thiserror::Error;

/// vicaya error type.
#[derive(Error, Debug)]
pub enum Error {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Index error: {0}")]
    Index(String),

    #[error("Scanner error: {0}")]
    Scanner(String),

    #[error("Watcher error: {0}")]
    Watcher(String),

    #[error("IPC error: {0}")]
    Ipc(String),

    #[error("{0}")]
    Other(String),
}

/// Result type alias for vicaya operations.
pub type Result<T> = std::result::Result<T, Error>;
