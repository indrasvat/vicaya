//! IPC client for communicating with the daemon.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use vicaya_core::ipc::{Request, Response};
use vicaya_index::SearchResult;

/// IPC client for daemon communication.
pub struct IpcClient {
    stream: Option<UnixStream>,
}

impl IpcClient {
    /// Create a new IPC client and attempt to connect.
    pub fn new() -> Self {
        let stream = Self::try_connect();
        Self { stream }
    }

    /// Try to connect to the daemon.
    fn try_connect() -> Option<UnixStream> {
        let socket_path = vicaya_core::ipc::socket_path();
        UnixStream::connect(&socket_path).ok()
    }

    /// Check if connected to daemon.
    pub fn is_connected(&self) -> bool {
        self.stream.is_some()
    }

    /// Reconnect to the daemon.
    pub fn reconnect(&mut self) {
        self.stream = Self::try_connect();
    }

    /// Search for files.
    pub fn search(&mut self, query: &str, limit: usize) -> anyhow::Result<Vec<SearchResult>> {
        if query.is_empty() {
            return Ok(Vec::new());
        }

        let req = Request::Search {
            query: query.to_string(),
            limit,
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
        // Try with existing connection first
        let mut stream = if let Some(stream) = self.stream.as_mut() {
            stream
        } else {
            // No connection, try to establish one
            self.reconnect();
            self.stream
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("Daemon not running"))?
        };

        // Serialize request
        let mut request_json = req
            .to_json()
            .map_err(|e| anyhow::anyhow!("Failed to serialize request: {}", e))?;
        request_json.push('\n');

        // Try to send request
        let result = stream.write_all(request_json.as_bytes());

        // If write failed, try reconnecting once
        if result.is_err() {
            self.reconnect();
            stream = self
                .stream
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("Daemon not running"))?;
            stream
                .write_all(request_json.as_bytes())
                .map_err(|e| anyhow::anyhow!("Failed to send request: {}", e))?;
        }

        // Read response
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| anyhow::anyhow!("Failed to read response: {}", e))?;

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
