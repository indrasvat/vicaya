//! IPC server for daemon communication.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use tracing::{debug, error, info};
use vicaya_core::ipc::{Request, Response};
use vicaya_core::{Config, Result};
use vicaya_index::{FileId, FileMeta, Query, QueryEngine};
use vicaya_scanner::{IndexSnapshot, Scanner};
use vicaya_watcher::IndexUpdate;

pub type SharedState = Arc<RwLock<DaemonState>>;

/// Shared daemon state.
pub struct DaemonState {
    pub config: Config,
    pub index_file: PathBuf,
    pub journal_file: PathBuf,
    pub snapshot: IndexSnapshot,
    pub path_to_id: std::collections::HashMap<String, FileId>,
    pub last_updated: i64,
}

impl DaemonState {
    pub fn new(
        config: Config,
        index_file: PathBuf,
        journal_file: PathBuf,
        snapshot: IndexSnapshot,
    ) -> Self {
        let path_to_id = build_path_map(&snapshot);
        let last_updated = index_file
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        Self {
            config,
            index_file,
            journal_file,
            snapshot,
            path_to_id,
            last_updated,
        }
    }

    pub fn apply_update(&mut self, update: IndexUpdate) {
        match update {
            IndexUpdate::Create { path } | IndexUpdate::Modify { path } => {
                self.upsert_path(Path::new(&path));
            }
            IndexUpdate::Delete { path } => {
                self.remove_path(Path::new(&path));
            }
            IndexUpdate::Move { from, to } => {
                self.move_path(Path::new(&from), Path::new(&to));
            }
        }
    }

    fn should_index(&self, path: &Path) -> bool {
        vicaya_core::filter::should_index_path(path, &self.config.exclusions)
    }

    fn upsert_path(&mut self, path: &Path) {
        if !self.should_index(path) {
            return;
        }

        let metadata = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(_) => return,
        };
        if !(metadata.is_file() || metadata.is_dir()) {
            return;
        }

        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;

        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let dev = metadata.dev();
        let ino = metadata.ino();
        let size = metadata.len();

        let path_str = path.to_string_lossy().to_string();
        let name_str = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let (path_offset, path_len) = self.snapshot.string_arena.add(&path_str);
        let (name_offset, name_len) = self.snapshot.string_arena.add(&name_str);

        let new_meta = FileMeta {
            path_offset,
            path_len,
            name_offset,
            name_len,
            size,
            mtime,
            dev,
            ino,
        };

        if let Some(&file_id) = self.path_to_id.get(&path_str) {
            let Some(meta) = self.snapshot.file_table.get_mut(file_id) else {
                return;
            };

            let old_name = self
                .snapshot
                .string_arena
                .get(meta.name_offset, meta.name_len)
                .unwrap_or("");

            if old_name != name_str {
                self.snapshot.trigram_index.remove_text(file_id, old_name);
                self.snapshot.trigram_index.add(file_id, &name_str);
            }

            *meta = new_meta;
        } else {
            let file_id = self.snapshot.file_table.insert(new_meta);
            self.snapshot.trigram_index.add(file_id, &name_str);
            self.path_to_id.insert(path_str, file_id);
        }

        self.last_updated = now_epoch_seconds();
    }

    fn remove_path(&mut self, path: &Path) {
        let path_str = path.to_string_lossy().to_string();
        let Some(file_id) = self.path_to_id.remove(&path_str) else {
            return;
        };

        self.tombstone_file(file_id);
    }

    fn tombstone_file(&mut self, file_id: FileId) {
        let Some(meta) = self.snapshot.file_table.get_mut(file_id) else {
            return;
        };

        let old_name = self
            .snapshot
            .string_arena
            .get(meta.name_offset, meta.name_len)
            .unwrap_or("");
        self.snapshot.trigram_index.remove_text(file_id, old_name);

        // Tombstone the entry (keeps IDs stable).
        meta.path_len = 0;
        meta.name_len = 0;
        meta.size = 0;
        meta.mtime = 0;

        self.last_updated = now_epoch_seconds();
    }

    fn move_path(&mut self, from: &Path, to: &Path) {
        let from_str = from.to_string_lossy().to_string();
        let Some(file_id) = self.path_to_id.remove(&from_str) else {
            // If we didn't know about the old path, treat as a create on the new path.
            self.upsert_path(to);
            return;
        };

        if !self.should_index(to) {
            self.tombstone_file(file_id);
            return;
        }

        let metadata = match std::fs::metadata(to) {
            Ok(m) => m,
            Err(_) => {
                self.tombstone_file(file_id);
                return;
            }
        };
        if !(metadata.is_file() || metadata.is_dir()) {
            self.tombstone_file(file_id);
            return;
        }

        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;

        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let dev = metadata.dev();
        let ino = metadata.ino();
        let size = metadata.len();

        let to_str = to.to_string_lossy().to_string();
        let name_str = to
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let Some(meta) = self.snapshot.file_table.get_mut(file_id) else {
            return;
        };

        let old_name = self
            .snapshot
            .string_arena
            .get(meta.name_offset, meta.name_len)
            .unwrap_or("");

        if old_name != name_str {
            self.snapshot.trigram_index.remove_text(file_id, old_name);
            self.snapshot.trigram_index.add(file_id, &name_str);
        }

        let (path_offset, path_len) = self.snapshot.string_arena.add(&to_str);
        let (name_offset, name_len) = self.snapshot.string_arena.add(&name_str);

        meta.path_offset = path_offset;
        meta.path_len = path_len;
        meta.name_offset = name_offset;
        meta.name_len = name_len;
        meta.size = size;
        meta.mtime = mtime;
        meta.dev = dev;
        meta.ino = ino;

        self.path_to_id.insert(to_str, file_id);
        self.last_updated = now_epoch_seconds();
    }
}

fn now_epoch_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn build_path_map(snapshot: &IndexSnapshot) -> std::collections::HashMap<String, FileId> {
    let mut map = std::collections::HashMap::with_capacity(snapshot.file_table.len());
    for (file_id, meta) in snapshot.file_table.iter() {
        if meta.path_len == 0 {
            continue;
        }
        if let Some(path) = snapshot.string_arena.get(meta.path_offset, meta.path_len) {
            map.insert(path.to_string(), file_id);
        }
    }
    map
}

/// IPC server that handles client connections.
pub struct IpcServer {
    listener: UnixListener,
    state: SharedState,
    shutdown: Arc<AtomicBool>,
    socket_path: PathBuf,
}

impl IpcServer {
    /// Create a new IPC server.
    pub fn new(socket_path: &Path, state: SharedState, shutdown: Arc<AtomicBool>) -> Result<Self> {
        // If a socket exists and is connectable, assume another daemon is already running.
        if socket_path.exists() {
            match UnixStream::connect(socket_path) {
                Ok(_) => {
                    return Err(vicaya_core::Error::Ipc(format!(
                        "Daemon already running (socket active at {})",
                        socket_path.display()
                    )));
                }
                Err(_) => {
                    // Stale socket, remove it.
                    let _ = std::fs::remove_file(socket_path);
                }
            }
        }

        // Ensure parent directory exists
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let listener = UnixListener::bind(socket_path)
            .map_err(|e| vicaya_core::Error::Ipc(format!("Failed to bind socket: {}", e)))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| vicaya_core::Error::Ipc(format!("Failed to set nonblocking: {}", e)))?;

        info!("IPC server listening on {}", socket_path.display());

        Ok(Self {
            listener,
            state,
            shutdown,
            socket_path: socket_path.to_path_buf(),
        })
    }

    /// Run the server loop.
    pub fn run(&self) -> Result<()> {
        while !self.shutdown.load(Ordering::Relaxed) {
            match self.listener.accept() {
                Ok((stream, _addr)) => self.handle_client(stream),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        }

        Ok(())
    }

    /// Handle a single client connection.
    fn handle_client(&self, mut stream: UnixStream) {
        let peer_addr = stream.peer_addr().ok();
        debug!("Client connected: {:?}", peer_addr);

        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();

        match reader.read_line(&mut line) {
            Ok(0) => debug!("Client disconnected"),
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
                let response = self.handle_request(request);
                self.send_response(&mut stream, &response);
            }
            Err(e) => error!("Failed to read from client: {}", e),
        }
    }

    /// Handle a request and generate a response.
    fn handle_request(&self, request: Request) -> Response {
        match request {
            Request::Search { query, limit } => {
                let state = self.state.read().unwrap();
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
                let state = self.state.read().unwrap();
                Response::Status {
                    pid: std::process::id() as i32,
                    indexed_files: state.path_to_id.len(),
                    trigram_count: state.snapshot.trigram_index.trigram_count(),
                    arena_size: state.snapshot.string_arena.size(),
                    last_updated: state.last_updated,
                }
            }
            Request::Rebuild { dry_run } => {
                let config = { self.state.read().unwrap().config.clone() };
                let index_file = { self.state.read().unwrap().index_file.clone() };
                let journal_file = { self.state.read().unwrap().journal_file.clone() };

                let scanner = Scanner::new(config.clone());
                let snapshot = match scanner.scan() {
                    Ok(s) => s,
                    Err(e) => {
                        error!("Rebuild failed: {}", e);
                        return Response::Error {
                            message: format!("Rebuild failed: {}", e),
                        };
                    }
                };

                let files_indexed = snapshot.file_table.len();

                if dry_run {
                    return Response::RebuildComplete { files_indexed };
                }

                if let Err(e) = snapshot.save(&index_file) {
                    error!("Failed to save rebuilt index: {}", e);
                    return Response::Error {
                        message: format!("Failed to save rebuilt index: {}", e),
                    };
                }

                // Clear journal after a successful full rebuild.
                let _ = std::fs::remove_file(&journal_file);

                let mut state = self.state.write().unwrap();
                state.snapshot = snapshot;
                state.path_to_id = build_path_map(&state.snapshot);
                state.last_updated = now_epoch_seconds();

                Response::RebuildComplete { files_indexed }
            }
            Request::Shutdown => {
                info!("Shutdown requested");
                self.shutdown.store(true, Ordering::Relaxed);
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

impl Drop for IpcServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket_path);
    }
}
