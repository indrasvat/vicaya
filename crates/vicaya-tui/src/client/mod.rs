//! IPC client for communicating with the daemon.

use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;
use vicaya_core::ipc::{Request, Response};
use vicaya_index::SearchResult;

const IPC_TIMEOUT: Duration = Duration::from_secs(2);
const REQUEST_ATTEMPTS: usize = 3;

/// IPC client for daemon communication.
pub struct IpcClient {
    stream: Option<UnixStream>,
    timeout: Duration,
    attempts: usize,
}

impl IpcClient {
    /// Create a new IPC client and attempt to connect.
    pub fn new() -> Self {
        Self::with_options(IPC_TIMEOUT, REQUEST_ATTEMPTS)
    }

    /// Create a best-effort IPC client for background health checks.
    pub fn best_effort() -> Self {
        Self::with_options(Duration::from_secs(1), 1)
    }

    fn with_options(timeout: Duration, attempts: usize) -> Self {
        let stream = Self::try_connect(timeout);
        Self {
            stream,
            timeout,
            attempts,
        }
    }

    /// Try to connect to the daemon.
    fn try_connect(timeout: Duration) -> Option<UnixStream> {
        let socket_path = vicaya_core::ipc::socket_path();
        let stream = UnixStream::connect(&socket_path).ok()?;
        let _ = stream.set_read_timeout(Some(timeout));
        let _ = stream.set_write_timeout(Some(timeout));
        Some(stream)
    }

    /// Check if connected to daemon.
    pub fn is_connected(&self) -> bool {
        self.stream.is_some()
    }

    /// Reconnect to the daemon.
    pub fn reconnect(&mut self) {
        self.stream = Self::try_connect(self.timeout);
    }

    /// Search for files.
    ///
    /// If `recent_if_empty` is true and `query` is empty, returns recent files by mtime.
    pub fn search(
        &mut self,
        query: &str,
        limit: usize,
        scope: Option<&std::path::Path>,
        filter_scope: Option<&std::path::Path>,
        recent_if_empty: bool,
    ) -> anyhow::Result<Vec<SearchResult>> {
        // If query is empty and we don't want recent files, return early
        if query.is_empty() && !recent_if_empty {
            return Ok(Vec::new());
        }

        let req = Request::Search {
            query: query.to_string(),
            limit,
            scope: scope.map(|p| p.to_string_lossy().to_string()),
            filter_scope: filter_scope.map(|p| p.to_string_lossy().to_string()),
            recent_if_empty,
        };

        match self.request(&req)? {
            Response::SearchResults { results } => {
                // Convert from vicaya_core::ipc::SearchResult to vicaya_index::SearchResult
                Ok(results
                    .into_iter()
                    .map(|r| SearchResult {
                        path: r.path,
                        name: r.name,
                        score: r.score,
                        size: r.size,
                        mtime: r.mtime,
                    })
                    .collect())
            }
            Response::Error { message } => Err(anyhow::anyhow!("Search error: {}", message)),
            _ => Err(anyhow::anyhow!("Unexpected response")),
        }
    }

    /// Get daemon status.
    pub fn status(&mut self) -> anyhow::Result<DaemonStatus> {
        let req = Request::Status;

        match self.request(&req)? {
            Response::Status {
                pid: _,
                build,
                indexed_files,
                trigram_count,
                arena_size,
                index_allocated_bytes: _,
                state_allocated_bytes: _,
                last_updated,
                reconciling,
            } => Ok(DaemonStatus {
                build,
                indexed_files,
                trigram_count,
                arena_size,
                last_updated,
                reconciling,
            }),
            Response::Error { message } => Err(anyhow::anyhow!("Status error: {}", message)),
            _ => Err(anyhow::anyhow!("Unexpected response")),
        }
    }

    /// Request index rebuild.
    pub fn rebuild(&mut self, dry_run: bool) -> anyhow::Result<usize> {
        let req = Request::Rebuild { dry_run };

        match self.request(&req)? {
            Response::RebuildComplete { files_indexed } => Ok(files_indexed),
            Response::Error { message } => Err(anyhow::anyhow!("Rebuild error: {}", message)),
            _ => Err(anyhow::anyhow!("Unexpected response")),
        }
    }

    /// Send a request and receive a response.
    fn request(&mut self, req: &Request) -> anyhow::Result<Response> {
        let mut request_json = req
            .to_json()
            .map_err(|e| anyhow::anyhow!("Failed to serialize request: {}", e))?;
        request_json.push('\n');

        let mut last_error: Option<anyhow::Error> = None;

        for attempt in 0..self.attempts {
            match self.request_once(&request_json) {
                Ok(response) => return Ok(response),
                Err(error) => {
                    last_error = Some(error);
                    self.stream = None;
                }
            }

            if attempt + 1 < self.attempts {
                std::thread::sleep(Duration::from_millis(50));
                self.stream = None;
                self.reconnect();
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Daemon not running")))
    }

    fn request_once(&mut self, request_json: &str) -> anyhow::Result<Response> {
        if self.stream.is_none() {
            self.reconnect();
        }

        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Daemon not running"))?;

        stream
            .write_all(request_json.as_bytes())
            .map_err(|e| anyhow::anyhow!("Failed to send request: {}", e))?;

        let mut reader = BufReader::new(stream);
        let line = vicaya_core::ipc::read_message(&mut reader)?
            .ok_or_else(|| anyhow::anyhow!("Daemon closed IPC connection"))?;

        Response::from_json(&line).map_err(|e| anyhow::anyhow!("Failed to parse response: {}", e))
    }
}

impl Default for IpcClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Daemon status information.
#[derive(Debug, Clone)]
pub struct DaemonStatus {
    pub build: vicaya_core::ipc::BuildInfo,
    pub indexed_files: usize,
    pub trigram_count: usize,
    pub arena_size: usize,
    pub last_updated: i64,
    pub reconciling: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Write};
    use std::os::unix::net::UnixListener;
    use vicaya_core::ipc::BuildInfo;

    fn response_server(
        dir: &std::path::Path,
        response: Response,
    ) -> std::thread::JoinHandle<Request> {
        let socket = dir.join("daemon.sock");
        let listener = UnixListener::bind(&socket).unwrap();

        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let line = vicaya_core::ipc::read_message(&mut reader)
                .unwrap()
                .unwrap();
            let request = Request::from_json(&line).unwrap();
            let mut json = response.to_json().unwrap();
            json.push('\n');
            stream.write_all(json.as_bytes()).unwrap();
            request
        })
    }

    fn close_then_response_server(
        dir: &std::path::Path,
        response: Response,
    ) -> std::thread::JoinHandle<Vec<Request>> {
        close_n_then_response_server(dir, 1, response)
    }

    fn close_n_then_response_server(
        dir: &std::path::Path,
        close_count: usize,
        response: Response,
    ) -> std::thread::JoinHandle<Vec<Request>> {
        let socket = dir.join("daemon.sock");
        let listener = UnixListener::bind(&socket).unwrap();

        std::thread::spawn(move || {
            let mut requests = Vec::new();

            for _ in 0..close_count {
                let (stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream);
                let line = vicaya_core::ipc::read_message(&mut reader)
                    .unwrap()
                    .unwrap();
                requests.push(Request::from_json(&line).unwrap());
                drop(reader);
            }

            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let line = vicaya_core::ipc::read_message(&mut reader)
                .unwrap()
                .unwrap();
            requests.push(Request::from_json(&line).unwrap());
            let mut json = response.to_json().unwrap();
            json.push('\n');
            stream.write_all(json.as_bytes()).unwrap();

            requests
        })
    }

    fn build_info() -> BuildInfo {
        BuildInfo {
            version: "1.2.0".to_string(),
            git_sha: "abc1234".to_string(),
            timestamp: "2026-05-19T00:00:00Z".to_string(),
            target: "aarch64-apple-darwin".to_string(),
        }
    }

    #[test]
    fn search_serializes_scope_and_maps_results() {
        let _lock = vicaya_core::paths::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("VICAYA_DIR", dir.path());
        let response = Response::SearchResults {
            results: vec![vicaya_core::ipc::SearchResult {
                path: "/tmp/repo/Cargo.toml".to_string(),
                name: "Cargo.toml".to_string(),
                score: 0.9,
                size: 123,
                mtime: 1_700_000_000,
            }],
        };
        let handle = response_server(dir.path(), response);

        let mut client = IpcClient::new();
        assert!(client.is_connected());
        let results = client
            .search(
                "Cargo",
                5,
                Some(std::path::Path::new("/tmp/repo")),
                Some(std::path::Path::new("/tmp/repo/src")),
                false,
            )
            .unwrap();

        let request = handle.join().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Cargo.toml");
        match request {
            Request::Search {
                query,
                limit,
                scope,
                filter_scope,
                recent_if_empty,
            } => {
                assert_eq!(query, "Cargo");
                assert_eq!(limit, 5);
                assert_eq!(scope.as_deref(), Some("/tmp/repo"));
                assert_eq!(filter_scope.as_deref(), Some("/tmp/repo/src"));
                assert!(!recent_if_empty);
            }
            other => panic!("unexpected request: {other:?}"),
        }
    }

    #[test]
    fn empty_non_recent_search_returns_without_ipc() {
        let _lock = vicaya_core::paths::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("VICAYA_DIR", dir.path());
        let mut client = IpcClient::best_effort();
        client.stream = None;
        let results = client.search("", 10, None, None, false).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn status_and_rebuild_map_daemon_responses() {
        let _lock = vicaya_core::paths::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("VICAYA_DIR", dir.path());
        let status_response = Response::Status {
            pid: 99,
            build: build_info(),
            indexed_files: 42,
            trigram_count: 777,
            arena_size: 4096,
            index_allocated_bytes: 8192,
            state_allocated_bytes: 16384,
            last_updated: 1_700_000_000,
            reconciling: true,
        };
        let handle = response_server(dir.path(), status_response);
        let mut client = IpcClient::new();
        let status = client.status().unwrap();
        let request = handle.join().unwrap();
        assert!(matches!(request, Request::Status));
        assert_eq!(status.indexed_files, 42);
        assert_eq!(status.trigram_count, 777);
        assert!(status.reconciling);

        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("VICAYA_DIR", dir.path());
        let handle = response_server(dir.path(), Response::RebuildComplete { files_indexed: 12 });
        let mut client = IpcClient::new();
        assert_eq!(client.rebuild(true).unwrap(), 12);
        assert!(matches!(
            handle.join().unwrap(),
            Request::Rebuild { dry_run: true }
        ));
    }

    #[test]
    fn request_reconnects_when_daemon_closes_stale_stream() {
        let _lock = vicaya_core::paths::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("VICAYA_DIR", dir.path());
        let handle = close_then_response_server(
            dir.path(),
            Response::Status {
                pid: 99,
                build: build_info(),
                indexed_files: 42,
                trigram_count: 777,
                arena_size: 4096,
                index_allocated_bytes: 8192,
                state_allocated_bytes: 16384,
                last_updated: 1_700_000_000,
                reconciling: false,
            },
        );

        let mut client = IpcClient::new();
        let status = client.status().unwrap();
        let requests = handle.join().unwrap();

        assert_eq!(status.indexed_files, 42);
        assert_eq!(requests.len(), 2);
        assert!(requests.iter().all(|req| matches!(req, Request::Status)));
    }

    #[test]
    fn request_survives_short_daemon_restart_window() {
        let _lock = vicaya_core::paths::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("VICAYA_DIR", dir.path());
        let handle = close_n_then_response_server(
            dir.path(),
            2,
            Response::SearchResults {
                results: vec![vicaya_core::ipc::SearchResult {
                    path: "/tmp/repo/main.rs".to_string(),
                    name: "main.rs".to_string(),
                    score: 1.0,
                    size: 12,
                    mtime: 1_700_000_000,
                }],
            },
        );

        let mut client = IpcClient::new();
        let results = client.search("main", 10, None, None, false).unwrap();
        let requests = handle.join().unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(requests.len(), 3);
        assert!(requests
            .iter()
            .all(|req| matches!(req, Request::Search { query, .. } if query == "main")));
    }

    #[test]
    fn daemon_error_responses_become_client_errors() {
        let _lock = vicaya_core::paths::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("VICAYA_DIR", dir.path());
        let handle = response_server(
            dir.path(),
            Response::Error {
                message: "boom".to_string(),
            },
        );
        let mut client = IpcClient::new();
        let err = client.search("x", 1, None, None, false).unwrap_err();
        assert!(err.to_string().contains("boom"));
        assert!(matches!(handle.join().unwrap(), Request::Search { .. }));
    }
}
