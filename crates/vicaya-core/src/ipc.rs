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
        /// Daemon process ID.
        #[serde(default)]
        pid: i32,
        indexed_files: usize,
        trigram_count: usize,
        arena_size: usize,
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

/// Get the socket path for IPC communication.
pub fn socket_path() -> std::path::PathBuf {
    crate::paths::socket_path()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        // Test Search request
        let search = Request::Search {
            query: "test".to_string(),
            limit: 10,
        };
        let json = search.to_json().unwrap();
        let decoded: Request = Request::from_json(&json).unwrap();
        assert!(
            matches!(decoded, Request::Search { query, limit } if query == "test" && limit == 10)
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
            indexed_files: 100,
            trigram_count: 500,
            arena_size: 2048,
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
}
