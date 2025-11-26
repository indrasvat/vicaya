//! IPC protocol for daemon communication.

use serde::{Deserialize, Serialize};

/// IPC request from client to daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Request {
    /// Search for files.
    Search { query: String, limit: usize },
    /// Get daemon status.
    Status,
    /// Trigger index rebuild.
    Rebuild { dry_run: bool },
    /// Shutdown the daemon.
    Shutdown,
}

/// IPC response from daemon to client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Response {
    /// Search results.
    SearchResults { results: Vec<SearchResult> },
    /// Status information.
    Status {
        indexed_files: usize,
        trigram_count: usize,
        arena_size: usize,
        last_updated: i64,
    },
    /// Rebuild completed.
    RebuildComplete { files_indexed: usize },
    /// Operation succeeded.
    Ok,
    /// Error occurred.
    Error { message: String },
}

/// A search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub path: String,
    pub name: String,
    pub score: f32,
    pub size: u64,
    pub mtime: i64,
}

impl Request {
    /// Serialize request to JSON.
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    /// Deserialize request from JSON.
    pub fn from_json(s: &str) -> serde_json::Result<Self> {
        serde_json::from_str(s)
    }
}

impl Response {
    /// Serialize response to JSON.
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    /// Deserialize response from JSON.
    pub fn from_json(s: &str) -> serde_json::Result<Self> {
        serde_json::from_str(s)
    }
}

/// Get the socket path for IPC communication.
pub fn socket_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("vicaya")
        .join("daemon.sock")
}
