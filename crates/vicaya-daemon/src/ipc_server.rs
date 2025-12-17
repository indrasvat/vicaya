//! IPC server for daemon communication.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
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
    pub inode_to_id: std::collections::HashMap<(u64, u64), FileId>,
    pub last_updated: i64,
    pub reconciling: bool,
}

impl DaemonState {
    pub fn new(
        config: Config,
        index_file: PathBuf,
        journal_file: PathBuf,
        snapshot: IndexSnapshot,
    ) -> Self {
        let path_to_id = build_path_map(&snapshot);
        let inode_to_id = build_inode_map(&snapshot);
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
            inode_to_id,
            last_updated,
            reconciling: false,
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

        let inode_key = (dev, ino);

        if let Some(&file_id) = self.path_to_id.get(&path_str) {
            let Some(meta) = self.snapshot.file_table.get_mut(file_id) else {
                return;
            };

            let old_inode_key = (meta.dev, meta.ino);
            if old_inode_key != inode_key {
                if self.inode_to_id.get(&old_inode_key) == Some(&file_id) {
                    self.inode_to_id.remove(&old_inode_key);
                }
                self.inode_to_id.insert(inode_key, file_id);
            }

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
        } else if let Some(&file_id) = self.inode_to_id.get(&inode_key) {
            // Same inode (dev+ino) already exists in the index under a different path; treat this
            // as a move/rename even if the watcher didn't report the old path.
            if let Some(meta) = self.snapshot.file_table.get_mut(file_id) {
                if let Some(old_path) = self
                    .snapshot
                    .string_arena
                    .get(meta.path_offset, meta.path_len)
                    .map(|s| s.to_string())
                {
                    self.path_to_id.remove(&old_path);
                }

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
                self.path_to_id.insert(path_str, file_id);
            }
        } else {
            let file_id = self.snapshot.file_table.insert(new_meta);
            self.snapshot.trigram_index.add(file_id, &name_str);
            self.path_to_id.insert(path_str, file_id);
            self.inode_to_id.insert(inode_key, file_id);
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

        let inode_key = (meta.dev, meta.ino);
        if inode_key != (0, 0) && self.inode_to_id.get(&inode_key) == Some(&file_id) {
            self.inode_to_id.remove(&inode_key);
        }

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

        let old_inode_key = (meta.dev, meta.ino);

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

        let new_inode_key = (dev, ino);
        if old_inode_key != new_inode_key {
            if self.inode_to_id.get(&old_inode_key) == Some(&file_id) {
                self.inode_to_id.remove(&old_inode_key);
            }
            self.inode_to_id.insert(new_inode_key, file_id);
        } else {
            self.inode_to_id.insert(new_inode_key, file_id);
        }

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

fn build_inode_map(snapshot: &IndexSnapshot) -> std::collections::HashMap<(u64, u64), FileId> {
    let mut map = std::collections::HashMap::with_capacity(snapshot.file_table.len());
    for (file_id, meta) in snapshot.file_table.iter() {
        if meta.path_len == 0 {
            continue;
        }
        if meta.dev == 0 && meta.ino == 0 {
            continue;
        }
        map.insert((meta.dev, meta.ino), file_id);
    }
    map
}

fn journal_len(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn read_journal_from_offset(path: &Path, offset: u64) -> Vec<IndexUpdate> {
    use std::io::Seek;

    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };

    let mut reader = BufReader::new(file);
    if offset > 0 {
        let _ = reader.seek(std::io::SeekFrom::Start(offset));
    }

    let mut updates = Vec::new();
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                error!("Failed to read journal line: {}", e);
                continue;
            }
        };

        match serde_json::from_str::<IndexUpdate>(&line) {
            Ok(update) => updates.push(update),
            Err(e) => error!("Skipping invalid journal entry: {}", e),
        }
    }

    updates
}

fn truncate_journal(path: &Path) -> std::io::Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)?;
    file.flush()?;
    Ok(())
}

pub fn full_rebuild_from_disk(
    state: &SharedState,
    journal_lock: &Arc<Mutex<()>>,
    rebuild_lock: &Arc<Mutex<()>>,
) -> Result<usize> {
    let _rebuild_guard = rebuild_lock.lock().unwrap();

    {
        let mut state = state.write().unwrap();
        state.reconciling = true;
    }

    let result = (|| {
        let (config, index_file, journal_file) = {
            let state = state.read().unwrap();
            (
                state.config.clone(),
                state.index_file.clone(),
                state.journal_file.clone(),
            )
        };

        let journal_offset = {
            let _guard = journal_lock.lock().unwrap();
            journal_len(&journal_file)
        };

        info!("Starting full index rebuild from disk...");
        let scanner = Scanner::new(config);
        let snapshot = scanner.scan()?;
        let files_indexed = snapshot.file_table.len();

        // Finalize: swap snapshot, apply any watcher updates that happened during the scan,
        // then persist a fresh snapshot and clear the journal.
        {
            let mut state = state.write().unwrap();
            let _journal_guard = journal_lock.lock().unwrap();

            let updates = read_journal_from_offset(&journal_file, journal_offset);

            state.snapshot = snapshot;
            state.path_to_id = build_path_map(&state.snapshot);
            state.inode_to_id = build_inode_map(&state.snapshot);
            state.last_updated = now_epoch_seconds();

            for update in updates {
                state.apply_update(update);
            }

            state.snapshot.save(&index_file)?;
            truncate_journal(&journal_file)?;

            state.reconciling = false;
        }

        info!("Full rebuild complete: {} files indexed", files_indexed);

        Ok(files_indexed)
    })();

    if result.is_err() {
        let mut state = state.write().unwrap();
        state.reconciling = false;
    }

    result
}

/// IPC server that handles client connections.
pub struct IpcServer {
    listener: UnixListener,
    state: SharedState,
    shutdown: Arc<AtomicBool>,
    socket_path: PathBuf,
    journal_lock: Arc<Mutex<()>>,
    rebuild_lock: Arc<Mutex<()>>,
}

impl IpcServer {
    /// Create a new IPC server.
    pub fn new(
        socket_path: &Path,
        state: SharedState,
        shutdown: Arc<AtomicBool>,
        journal_lock: Arc<Mutex<()>>,
        rebuild_lock: Arc<Mutex<()>>,
    ) -> Result<Self> {
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
            journal_lock,
            rebuild_lock,
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
                    reconciling: state.reconciling,
                }
            }
            Request::Rebuild { dry_run } => {
                if dry_run {
                    let config = { self.state.read().unwrap().config.clone() };
                    let scanner = Scanner::new(config);
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
                    return Response::RebuildComplete { files_indexed };
                }

                match full_rebuild_from_disk(&self.state, &self.journal_lock, &self.rebuild_lock) {
                    Ok(files_indexed) => Response::RebuildComplete { files_indexed },
                    Err(e) => Response::Error {
                        message: format!("Rebuild failed: {}", e),
                    },
                }
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
