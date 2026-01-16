//! IPC server for daemon communication.

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
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
    pub path_hasher: RandomState,
    pub path_to_id: std::collections::HashMap<u64, FileId>,
    pub path_hash_collisions: std::collections::HashMap<u64, Vec<FileId>>,
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
        let path_hasher = RandomState::new();
        let (path_to_id, path_hash_collisions) = build_path_map(&snapshot, &path_hasher);
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
            path_hasher,
            path_to_id,
            path_hash_collisions,
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

    fn indexed_file_count(&self) -> usize {
        self.path_to_id.len()
            + self
                .path_hash_collisions
                .values()
                .map(|ids| ids.len())
                .sum::<usize>()
    }

    fn estimated_index_allocated_bytes(&self) -> u64 {
        (self.snapshot.file_table.allocated_bytes()
            + self.snapshot.string_arena.allocated_bytes()
            + self.snapshot.trigram_index.allocated_bytes()) as u64
    }

    fn estimated_state_allocated_bytes(&self) -> u64 {
        let collisions_vec_bytes: usize = self
            .path_hash_collisions
            .values()
            .map(|ids| ids.capacity() * std::mem::size_of::<FileId>())
            .sum();

        (self.estimated_index_allocated_bytes() as usize
            + hash_map_allocated_bytes(&self.path_to_id)
            + hash_map_allocated_bytes(&self.path_hash_collisions)
            + collisions_vec_bytes
            + hash_map_allocated_bytes(&self.inode_to_id)) as u64
    }

    fn path_hash(&self, path: &str) -> u64 {
        self.path_hasher.hash_one(path)
    }

    fn file_id_matches_path(&self, file_id: FileId, path: &str) -> bool {
        let Some(meta) = self.snapshot.file_table.get(file_id) else {
            return false;
        };
        let Some(existing_path) = self
            .snapshot
            .string_arena
            .get(meta.path_offset, meta.path_len)
        else {
            return false;
        };
        existing_path == path
    }

    fn get_file_id_for_path(&self, path: &str) -> Option<FileId> {
        let hash = self.path_hash(path);

        if let Some(ids) = self.path_hash_collisions.get(&hash) {
            return ids
                .iter()
                .copied()
                .find(|&file_id| self.file_id_matches_path(file_id, path));
        }

        let file_id = self.path_to_id.get(&hash).copied()?;
        self.file_id_matches_path(file_id, path).then_some(file_id)
    }

    fn insert_path_mapping(&mut self, path: &str, file_id: FileId) {
        let hash = self.path_hash(path);

        if let Some(ids) = self.path_hash_collisions.get_mut(&hash) {
            ids.push(file_id);
            return;
        }

        let Some(existing) = self.path_to_id.get(&hash).copied() else {
            self.path_to_id.insert(hash, file_id);
            return;
        };

        if self.file_id_matches_path(existing, path) {
            self.path_to_id.insert(hash, file_id);
            return;
        }

        self.path_to_id.remove(&hash);
        self.path_hash_collisions
            .insert(hash, vec![existing, file_id]);
    }

    fn remove_path_mapping(&mut self, path: &str) -> Option<FileId> {
        let hash = self.path_hash(path);
        let snapshot = &self.snapshot;

        if let Some(ids) = self.path_hash_collisions.get_mut(&hash) {
            let pos = ids.iter().position(|&file_id| {
                let Some(meta) = snapshot.file_table.get(file_id) else {
                    return false;
                };
                let Some(existing_path) =
                    snapshot.string_arena.get(meta.path_offset, meta.path_len)
                else {
                    return false;
                };
                existing_path == path
            })?;
            let file_id = ids.remove(pos);

            match ids.len() {
                0 => {
                    self.path_hash_collisions.remove(&hash);
                }
                1 => {
                    let remaining = ids[0];
                    self.path_hash_collisions.remove(&hash);
                    self.path_to_id.insert(hash, remaining);
                }
                _ => {}
            }

            return Some(file_id);
        }

        let file_id = self.path_to_id.get(&hash).copied()?;
        if !self.file_id_matches_path(file_id, path) {
            return None;
        }

        self.path_to_id.remove(&hash);
        Some(file_id)
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

        let path_str = path.to_string_lossy();
        let name_str = path
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        if name_str.is_empty() {
            return;
        }

        let inode_key = (dev, ino);

        if let Some(file_id) = self.get_file_id_for_path(path_str.as_ref()) {
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

            if old_name != name_str.as_ref() {
                self.snapshot.trigram_index.remove_text(file_id, old_name);
                self.snapshot.trigram_index.add(file_id, name_str.as_ref());

                let (name_offset, name_len) = self.snapshot.string_arena.add(name_str.as_ref());
                meta.name_offset = name_offset;
                meta.name_len = name_len;
            }

            meta.size = size;
            meta.mtime = mtime;
            meta.dev = dev;
            meta.ino = ino;
        } else if let Some(&file_id) = self.inode_to_id.get(&inode_key) {
            // Same inode (dev+ino) already exists in the index under a different path; treat this
            // as a move/rename even if the watcher didn't report the old path.
            let (old_path, old_name) = {
                let Some(meta) = self.snapshot.file_table.get(file_id) else {
                    return;
                };

                let old_path = self
                    .snapshot
                    .string_arena
                    .get(meta.path_offset, meta.path_len)
                    .unwrap_or("")
                    .to_string();

                let old_name = self
                    .snapshot
                    .string_arena
                    .get(meta.name_offset, meta.name_len)
                    .unwrap_or("")
                    .to_string();

                (old_path, old_name)
            };

            if !old_path.is_empty() {
                let _ = self.remove_path_mapping(&old_path);
            }

            let Some(meta) = self.snapshot.file_table.get_mut(file_id) else {
                return;
            };

            if old_name != name_str.as_ref() {
                self.snapshot.trigram_index.remove_text(file_id, &old_name);
                self.snapshot.trigram_index.add(file_id, name_str.as_ref());

                let (name_offset, name_len) = self.snapshot.string_arena.add(name_str.as_ref());
                meta.name_offset = name_offset;
                meta.name_len = name_len;
            }

            let (path_offset, path_len) = self.snapshot.string_arena.add(path_str.as_ref());
            meta.path_offset = path_offset;
            meta.path_len = path_len;
            meta.size = size;
            meta.mtime = mtime;
            meta.dev = dev;
            meta.ino = ino;

            self.insert_path_mapping(path_str.as_ref(), file_id);
        } else {
            let (path_offset, path_len) = self.snapshot.string_arena.add(path_str.as_ref());
            let (name_offset, name_len) = self.snapshot.string_arena.add(name_str.as_ref());

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

            let file_id = self.snapshot.file_table.insert(new_meta);
            self.snapshot.trigram_index.add(file_id, name_str.as_ref());
            self.insert_path_mapping(path_str.as_ref(), file_id);
            self.inode_to_id.insert(inode_key, file_id);
        }

        self.last_updated = now_epoch_seconds();
    }

    fn remove_path(&mut self, path: &Path) {
        let path_str = path.to_string_lossy();
        let Some(file_id) = self.remove_path_mapping(path_str.as_ref()) else {
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
        let from_str = from.to_string_lossy();
        let Some(file_id) = self.remove_path_mapping(from_str.as_ref()) else {
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

        let to_str = to.to_string_lossy();
        let name_str = to
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        if name_str.is_empty() {
            self.tombstone_file(file_id);
            return;
        }

        let Some(meta) = self.snapshot.file_table.get_mut(file_id) else {
            return;
        };

        let old_inode_key = (meta.dev, meta.ino);

        let old_name = self
            .snapshot
            .string_arena
            .get(meta.name_offset, meta.name_len)
            .unwrap_or("");

        if old_name != name_str.as_ref() {
            self.snapshot.trigram_index.remove_text(file_id, old_name);
            self.snapshot.trigram_index.add(file_id, name_str.as_ref());

            let (name_offset, name_len) = self.snapshot.string_arena.add(name_str.as_ref());
            meta.name_offset = name_offset;
            meta.name_len = name_len;
        }

        let (path_offset, path_len) = self.snapshot.string_arena.add(to_str.as_ref());

        meta.path_offset = path_offset;
        meta.path_len = path_len;
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

        self.insert_path_mapping(to_str.as_ref(), file_id);
        self.last_updated = now_epoch_seconds();
    }
}

fn now_epoch_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn hash_map_allocated_bytes<K, V>(map: &std::collections::HashMap<K, V>) -> usize {
    // `HashMap` allocates a contiguous bucket array plus a control byte array.
    // We approximate control bytes as 1 byte per bucket (hashbrown SwissTable style).
    map.capacity() * std::mem::size_of::<(K, V)>() + map.capacity()
}

fn snapshot_path_for_id(snapshot: &IndexSnapshot, file_id: FileId) -> Option<&str> {
    let meta = snapshot.file_table.get(file_id)?;
    snapshot.string_arena.get(meta.path_offset, meta.path_len)
}

fn build_path_map(
    snapshot: &IndexSnapshot,
    hasher: &RandomState,
) -> (
    std::collections::HashMap<u64, FileId>,
    std::collections::HashMap<u64, Vec<FileId>>,
) {
    let mut map = std::collections::HashMap::with_capacity(snapshot.file_table.len());
    let mut collisions = std::collections::HashMap::<u64, Vec<FileId>>::new();

    for (file_id, meta) in snapshot.file_table.iter() {
        if meta.path_len == 0 {
            continue;
        }

        let Some(path) = snapshot.string_arena.get(meta.path_offset, meta.path_len) else {
            continue;
        };

        let hash = hasher.hash_one(path);

        if let Some(ids) = collisions.get_mut(&hash) {
            ids.push(file_id);
            continue;
        }

        let Some(existing) = map.get(&hash).copied() else {
            map.insert(hash, file_id);
            continue;
        };

        let existing_path = snapshot_path_for_id(snapshot, existing).unwrap_or("");
        if existing_path == path {
            // Duplicate path (unexpected); prefer the latest ID deterministically.
            map.insert(hash, file_id);
            continue;
        }

        map.remove(&hash);
        collisions.insert(hash, vec![existing, file_id]);
    }

    (map, collisions)
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

fn apply_journal_from_offset<F>(path: &Path, offset: u64, mut apply: F) -> usize
where
    F: FnMut(IndexUpdate),
{
    use std::io::{BufRead, Seek};

    let Ok(file) = std::fs::File::open(path) else {
        return 0;
    };

    let mut reader = BufReader::new(file);
    if offset > 0 {
        let _ = reader.seek(std::io::SeekFrom::Start(offset));
    }

    let mut applied = 0usize;
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                error!("Failed to read journal line: {}", e);
                continue;
            }
        }

        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }

        match serde_json::from_str::<IndexUpdate>(trimmed) {
            Ok(update) => {
                apply(update);
                applied += 1;
            }
            Err(e) => error!("Skipping invalid journal entry: {}", e),
        }
    }

    applied
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

            state.snapshot = snapshot;
            let (path_to_id, path_hash_collisions) =
                build_path_map(&state.snapshot, &state.path_hasher);
            state.path_to_id = path_to_id;
            state.path_hash_collisions = path_hash_collisions;
            state.inode_to_id = build_inode_map(&state.snapshot);
            state.last_updated = now_epoch_seconds();

            let applied_updates = apply_journal_from_offset(&journal_file, journal_offset, |u| {
                state.apply_update(u);
            });
            if applied_updates > 0 {
                debug!("Applied {} journal updates after rebuild", applied_updates);
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
                Ok((stream, _addr)) => {
                    // The listener is non-blocking, but client streams should be blocking so we can
                    // read full newline-delimited requests and write full JSON responses.
                    if let Err(e) = stream.set_nonblocking(false) {
                        error!("Failed to set client stream blocking mode: {}", e);
                    }
                    self.handle_client(stream);
                }
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
            Request::Search {
                query,
                limit,
                scope,
                recent_if_empty,
            } => {
                let state = self.state.read().unwrap();
                let engine = QueryEngine::new(
                    &state.snapshot.file_table,
                    &state.snapshot.string_arena,
                    &state.snapshot.trigram_index,
                );

                let scope_path = scope
                    .filter(|s| !s.trim().is_empty())
                    .map(std::path::PathBuf::from);

                // If query is empty and recent_if_empty is true, return recent files
                let results = if query.trim().is_empty() && recent_if_empty {
                    engine.recent_files(limit, scope_path.as_deref())
                } else {
                    let query_obj = Query {
                        term: query,
                        limit,
                        scope: scope_path,
                    };
                    engine.search(&query_obj)
                };

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
                    build: vicaya_core::ipc::BuildInfo {
                        version: vicaya_core::build_info::BUILD_INFO.version.to_string(),
                        git_sha: vicaya_core::build_info::BUILD_INFO.git_sha.to_string(),
                        timestamp: vicaya_core::build_info::BUILD_INFO.timestamp.to_string(),
                        target: vicaya_core::build_info::BUILD_INFO.target.to_string(),
                    },
                    indexed_files: state.indexed_file_count(),
                    trigram_count: state.snapshot.trigram_index.trigram_count(),
                    arena_size: state.snapshot.string_arena.size(),
                    index_allocated_bytes: state.estimated_index_allocated_bytes(),
                    state_allocated_bytes: state.estimated_state_allocated_bytes(),
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
