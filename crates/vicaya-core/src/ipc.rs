//! IPC protocol for daemon communication.

use std::io::BufRead;

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Maximum newline-delimited IPC message size in bytes.
pub const MAX_IPC_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

/// Build metadata for a running daemon or client.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuildInfo {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub git_sha: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub target: String,
}

/// IPC request from client to daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Request {
    /// Search for files.
    Search {
        query: String,
        limit: usize,
        /// Optional scope root (directory path) used to boost results "near" the user's context.
        #[serde(default)]
        scope: Option<String>,
        /// When true and query is empty, return recent files instead of empty results.
        #[serde(default)]
        recent_if_empty: bool,
    },
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
        /// Daemon process ID.
        #[serde(default)]
        pid: i32,
        /// Daemon build metadata (useful to detect client/daemon mismatches).
        #[serde(default)]
        build: BuildInfo,
        indexed_files: usize,
        trigram_count: usize,
        arena_size: usize,
        /// Approximate heap bytes used by index structures.
        #[serde(default)]
        index_allocated_bytes: u64,
        /// Approximate heap bytes used by daemon state (index + maps).
        #[serde(default)]
        state_allocated_bytes: u64,
        last_updated: i64,
        /// Whether the daemon is currently rebuilding/reconciling the index.
        #[serde(default)]
        reconciling: bool,
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

/// Read one newline-delimited IPC message without unbounded allocation.
///
/// Returns `Ok(None)` on clean EOF before any bytes are read. If EOF arrives
/// after some bytes but before a newline, the partial message is returned so
/// callers can decide whether it is valid.
pub fn read_message<R: BufRead>(reader: &mut R) -> Result<Option<String>> {
    let mut message = Vec::new();

    loop {
        let available = reader
            .fill_buf()
            .map_err(|e| Error::Ipc(format!("Failed to read IPC message: {}", e)))?;

        if available.is_empty() {
            if message.is_empty() {
                return Ok(None);
            }
            return String::from_utf8(message)
                .map(Some)
                .map_err(|e| Error::Ipc(format!("IPC message was not valid UTF-8: {}", e)));
        }

        let take = match available.iter().position(|&byte| byte == b'\n') {
            Some(newline_idx) => newline_idx + 1,
            None => available.len(),
        };

        if message.len() + take > MAX_IPC_MESSAGE_BYTES {
            return Err(Error::Ipc(format!(
                "IPC message exceeds {} bytes",
                MAX_IPC_MESSAGE_BYTES
            )));
        }

        message.extend_from_slice(&available[..take]);
        reader.consume(take);

        if take > 0 && message.last() == Some(&b'\n') {
            return String::from_utf8(message)
                .map(Some)
                .map_err(|e| Error::Ipc(format!("IPC message was not valid UTF-8: {}", e)));
        }
    }
}

/// Get the socket path for IPC communication.
pub fn socket_path() -> std::path::PathBuf {
    crate::paths::socket_path()
}

#[cfg(test)]
mod tests {
    use std::io::BufReader;

    use super::*;

    #[test]
    fn test_request_serialization() {
        // Test Search request
        let search = Request::Search {
            query: "test".to_string(),
            limit: 10,
            scope: None,
            recent_if_empty: false,
        };
        let json = search.to_json().unwrap();
        let decoded: Request = Request::from_json(&json).unwrap();
        assert!(
            matches!(decoded, Request::Search { query, limit, scope, recent_if_empty } if query == "test" && limit == 10 && scope.is_none() && !recent_if_empty)
        );

        // Test Status request
        let status = Request::Status;
        let json = status.to_json().unwrap();
        let decoded = Request::from_json(&json).unwrap();
        assert!(matches!(decoded, Request::Status));

        // Test Rebuild request
        let rebuild = Request::Rebuild { dry_run: true };
        let json = rebuild.to_json().unwrap();
        let decoded = Request::from_json(&json).unwrap();
        assert!(matches!(decoded, Request::Rebuild { dry_run: true }));

        // Test Shutdown request
        let shutdown = Request::Shutdown;
        let json = shutdown.to_json().unwrap();
        let decoded = Request::from_json(&json).unwrap();
        assert!(matches!(decoded, Request::Shutdown));
    }

    #[test]
    fn test_response_serialization() {
        // Test SearchResults response
        let results = Response::SearchResults {
            results: vec![SearchResult {
                path: "/test/file.rs".to_string(),
                name: "file.rs".to_string(),
                score: 0.95,
                size: 1024,
                mtime: 1234567890,
            }],
        };
        let json = results.to_json().unwrap();
        let decoded = Response::from_json(&json).unwrap();
        assert!(matches!(decoded, Response::SearchResults { .. }));

        // Test Status response
        let status = Response::Status {
            pid: 123,
            build: BuildInfo::default(),
            indexed_files: 100,
            trigram_count: 500,
            arena_size: 2048,
            index_allocated_bytes: 0,
            state_allocated_bytes: 0,
            last_updated: 1234567890,
            reconciling: false,
        };
        let json = status.to_json().unwrap();
        let decoded = Response::from_json(&json).unwrap();
        assert!(matches!(
            decoded,
            Response::Status {
                pid: 123,
                indexed_files: 100,
                ..
            }
        ));

        // Test Ok response
        let ok = Response::Ok;
        let json = ok.to_json().unwrap();
        let decoded = Response::from_json(&json).unwrap();
        assert!(matches!(decoded, Response::Ok));

        // Test Error response
        let error = Response::Error {
            message: "test error".to_string(),
        };
        let json = error.to_json().unwrap();
        let decoded = Response::from_json(&json).unwrap();
        assert!(matches!(decoded, Response::Error { message } if message == "test error"));
    }

    #[test]
    fn test_invalid_json() {
        // Test invalid JSON
        let result = Request::from_json("invalid json");
        assert!(result.is_err());

        let result = Response::from_json("{\"invalid\": \"json\"}");
        assert!(result.is_err());
    }

    #[test]
    fn test_socket_path() {
        let path = socket_path();
        assert!(path.to_string_lossy().ends_with("daemon.sock"));
    }

    #[test]
    fn test_search_result_fields() {
        let result = SearchResult {
            path: "/home/user/test.rs".to_string(),
            name: "test.rs".to_string(),
            score: 1.0,
            size: 2048,
            mtime: 1234567890,
        };

        assert_eq!(result.path, "/home/user/test.rs");
        assert_eq!(result.name, "test.rs");
        assert_eq!(result.score, 1.0);
        assert_eq!(result.size, 2048);
        assert_eq!(result.mtime, 1234567890);
    }

    #[test]
    fn test_read_message_under_limit_with_newline() {
        let mut reader = BufReader::new(&b"{\"type\":\"status\"}\n"[..]);
        let message = read_message(&mut reader).unwrap();
        assert_eq!(message.as_deref(), Some("{\"type\":\"status\"}\n"));
    }

    #[test]
    fn test_read_message_under_limit_without_newline() {
        let mut reader = BufReader::new(&b"{\"type\":\"status\"}"[..]);
        let message = read_message(&mut reader).unwrap();
        assert_eq!(message.as_deref(), Some("{\"type\":\"status\"}"));
    }

    #[test]
    fn test_read_message_rejects_payload_over_limit_without_newline() {
        let oversized = vec![b'a'; MAX_IPC_MESSAGE_BYTES + 1];
        let mut reader = BufReader::new(oversized.as_slice());
        let err = read_message(&mut reader).unwrap_err();
        assert!(matches!(err, Error::Ipc(message) if message.contains("exceeds")));
    }

    #[test]
    fn test_read_message_rejects_payload_over_limit_before_newline() {
        let mut oversized = vec![b'a'; MAX_IPC_MESSAGE_BYTES];
        oversized.push(b'\n');
        let mut reader = BufReader::new(oversized.as_slice());
        let err = read_message(&mut reader).unwrap_err();
        assert!(matches!(err, Error::Ipc(message) if message.contains("exceeds")));
    }
}
