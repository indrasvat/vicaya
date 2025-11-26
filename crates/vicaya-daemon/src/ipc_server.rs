//! IPC server for daemon communication.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use tracing::{debug, error, info, warn};
use vicaya_core::ipc::{Request, Response};
use vicaya_core::Result;
use vicaya_index::{Query, QueryEngine};
use vicaya_scanner::IndexSnapshot;

/// Shared daemon state.
pub struct DaemonState {
    pub snapshot: IndexSnapshot,
}

/// IPC server that handles client connections.
pub struct IpcServer {
    listener: UnixListener,
    state: Arc<Mutex<DaemonState>>,
}

impl IpcServer {
    /// Create a new IPC server.
    pub fn new(socket_path: &std::path::Path, snapshot: IndexSnapshot) -> Result<Self> {
        // Remove existing socket if it exists
        if socket_path.exists() {
            std::fs::remove_file(socket_path)?;
        }

        // Ensure parent directory exists
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let listener = UnixListener::bind(socket_path)
            .map_err(|e| vicaya_core::Error::Ipc(format!("Failed to bind socket: {}", e)))?;

        info!("IPC server listening on {}", socket_path.display());

        Ok(Self {
            listener,
            state: Arc::new(Mutex::new(DaemonState { snapshot })),
        })
    }

    /// Run the server loop.
    pub fn run(&self) -> Result<()> {
        for stream in self.listener.incoming() {
            match stream {
                Ok(stream) => {
                    let state = Arc::clone(&self.state);
                    self.handle_client(stream, state);
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                }
            }
        }
        Ok(())
    }

    /// Handle a single client connection.
    fn handle_client(&self, mut stream: UnixStream, state: Arc<Mutex<DaemonState>>) {
        let peer_addr = stream.peer_addr().ok();
        debug!("Client connected: {:?}", peer_addr);

        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();

        match reader.read_line(&mut line) {
            Ok(0) => {
                debug!("Client disconnected");
            }
            Ok(_) => {
                let request = match Request::from_json(&line) {
                    Ok(req) => req,
                    Err(e) => {
                        error!("Failed to parse request: {}", e);
                        let response = Response::Error {
                            message: format!("Invalid request: {}", e),
                        };
                        self.send_response(&mut stream, &response);
                        return;
                    }
                };

                debug!("Received request: {:?}", request);
                let response = self.handle_request(request, state);
                self.send_response(&mut stream, &response);
            }
            Err(e) => {
                error!("Failed to read from client: {}", e);
            }
        }
    }

    /// Handle a request and generate a response.
    fn handle_request(&self, request: Request, state: Arc<Mutex<DaemonState>>) -> Response {
        match request {
            Request::Search { query, limit } => {
                let state = state.lock().unwrap();
                let engine = QueryEngine::new(
                    &state.snapshot.file_table,
                    &state.snapshot.string_arena,
                    &state.snapshot.trigram_index,
                );

                let query_obj = Query { term: query, limit };

                let results = engine.search(&query_obj);
                let ipc_results = results
                    .into_iter()
                    .map(|r| vicaya_core::ipc::SearchResult {
                        path: r.path,
                        name: r.name,
                        score: r.score,
                        size: r.size,
                        mtime: r.mtime,
                    })
                    .collect();

                Response::SearchResults {
                    results: ipc_results,
                }
            }
            Request::Status => {
                let state = state.lock().unwrap();
                Response::Status {
                    indexed_files: state.snapshot.file_table.len(),
                    trigram_count: state.snapshot.trigram_index.trigram_count(),
                    arena_size: state.snapshot.string_arena.size(),
                    last_updated: 0, // TODO: track this
                }
            }
            Request::Rebuild { dry_run: _ } => {
                // TODO: Implement rebuild
                warn!("Rebuild not yet implemented via IPC");
                Response::Error {
                    message: "Rebuild not yet implemented".to_string(),
                }
            }
            Request::Shutdown => {
                info!("Shutdown requested");
                Response::Ok
            }
        }
    }

    /// Send a response to the client.
    fn send_response(&self, stream: &mut UnixStream, response: &Response) {
        match response.to_json() {
            Ok(json) => {
                let mut data = json;
                data.push('\n');
                if let Err(e) = stream.write_all(data.as_bytes()) {
                    error!("Failed to send response: {}", e);
                }
            }
            Err(e) => {
                error!("Failed to serialize response: {}", e);
            }
        }
    }
}
