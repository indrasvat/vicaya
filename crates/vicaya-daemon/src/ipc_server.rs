//! IPC server for daemon communication.

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::io::{BufReader, Write};
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
const RECENT_UPDATE_LIMIT: usize = 4096;

/// Shared daemon state.
pub struct DaemonState {
    pub config: Config,
    pub index_file: PathBuf,
    pub journal_file: PathBuf,
    pub snapshot: IndexSnapshot,
    pub path_hasher: RandomState,
    pub path_to_id: std::collections::HashMap<u64, FileId>,
    pub path_hash_collisions: std::collections::HashMap<u64, Vec<FileId>>,
    pub path_order: Vec<FileId>,
    pub path_order_dirty: bool,
    pub name_to_ids: std::collections::HashMap<String, Vec<FileId>>,
    pub recent_order: Vec<FileId>,
    pub recent_updates: Vec<FileId>,
    pub inode_to_id: std::collections::HashMap<(u64, u64), FileId>,
    pub last_updated: i64,
    pub reconciling: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum PreparedIndexUpdate {
    CreateOrModify {
        file: Option<PreparedFileMeta>,
    },
    Delete {
        path: PathBuf,
    },
    Move {
        from: PathBuf,
        file: Option<PreparedFileMeta>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedFileMeta {
    path: String,
    name: String,
    size: u64,
    mtime: i64,
    dev: u64,
    ino: u64,
}

pub(crate) fn prepare_index_update(config: &Config, update: IndexUpdate) -> PreparedIndexUpdate {
    match update {
        IndexUpdate::Create { path } | IndexUpdate::Modify { path } => {
            let path = PathBuf::from(path);
            PreparedIndexUpdate::CreateOrModify {
                file: prepare_file_meta(config, &path),
            }
        }
        IndexUpdate::Delete { path } => PreparedIndexUpdate::Delete {
            path: PathBuf::from(path),
        },
        IndexUpdate::Move { from, to } => {
            let to = PathBuf::from(to);
            PreparedIndexUpdate::Move {
                from: PathBuf::from(from),
                file: prepare_file_meta(config, &to),
            }
        }
    }
}

fn prepare_file_meta(config: &Config, path: &Path) -> Option<PreparedFileMeta> {
    let metadata = std::fs::metadata(path).ok()?;
    if !(metadata.is_file() || metadata.is_dir()) {
        return None;
    }

    if !vicaya_scanner::should_index_path(config, path, metadata.is_dir()) {
        return None;
    }

    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;

    let name = path.file_name()?.to_string_lossy().to_string();
    if name.is_empty() {
        return None;
    }

    Some(PreparedFileMeta {
        path: path.to_string_lossy().to_string(),
        name,
        size: metadata.len(),
        mtime: metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
        dev: metadata.dev(),
        ino: metadata.ino(),
    })
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
        let path_order = build_path_order(&snapshot);
        let name_to_ids = build_name_map(&snapshot);
        let recent_order = build_recent_order(&snapshot);
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
            path_order,
            path_order_dirty: false,
            name_to_ids,
            recent_order,
            recent_updates: Vec::new(),
            inode_to_id,
            last_updated,
            reconciling: false,
        }
    }

    pub fn apply_update(&mut self, update: IndexUpdate) {
        let update = prepare_index_update(&self.config, update);
        self.apply_prepared_update(update);
    }

    pub(crate) fn apply_prepared_update(&mut self, update: PreparedIndexUpdate) {
        match update {
            PreparedIndexUpdate::CreateOrModify { file } => {
                if let Some(file) = file {
                    self.upsert_prepared(file);
                }
            }
            PreparedIndexUpdate::Delete { path } => {
                self.remove_path(&path);
            }
            PreparedIndexUpdate::Move { from, file } => {
                self.move_prepared(&from, file);
            }
        }
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
            + self.path_order.capacity() * std::mem::size_of::<FileId>()
            + hash_map_allocated_bytes(&self.name_to_ids)
            + self
                .name_to_ids
                .values()
                .map(|ids| ids.capacity() * std::mem::size_of::<FileId>())
                .sum::<usize>()
            + self.recent_order.capacity() * std::mem::size_of::<FileId>()
            + self.recent_updates.capacity() * std::mem::size_of::<FileId>()
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

    fn mark_path_order_dirty(&mut self) {
        self.path_order_dirty = true;
    }

    fn scoped_file_ids_up_to(&self, scope: &Path, max_ids: usize) -> Option<(Vec<FileId>, bool)> {
        if self.path_order_dirty {
            return None;
        }

        let (scope, scope_child_prefix) = normalized_scope_parts(scope)?;
        let start = self.path_order.partition_point(|&id| {
            snapshot_path_for_id(&self.snapshot, id).is_some_and(|path| path < scope.as_str())
        });

        let mut ids = Vec::new();
        for &file_id in &self.path_order[start..] {
            let Some(path) = snapshot_path_for_id(&self.snapshot, file_id) else {
                continue;
            };
            if path_is_in_normalized_scope(path, &scope, &scope_child_prefix) {
                ids.push(file_id);
                if ids.len() > max_ids {
                    return Some((ids, false));
                }
                continue;
            }
            if path > scope.as_str() && !path.starts_with(&scope_child_prefix) {
                break;
            }
        }

        Some((ids, true))
    }

    fn recent_file_ids(&self, limit: usize, scope: Option<&Path>) -> Option<Vec<FileId>> {
        let scope = scope.and_then(normalized_scope_parts);
        let mut seen = std::collections::HashSet::with_capacity(limit.saturating_mul(2));
        let mut ids = Vec::with_capacity(limit);

        for file_id in self
            .recent_updates
            .iter()
            .rev()
            .chain(self.recent_order.iter())
            .copied()
        {
            if ids.len() == limit {
                break;
            }
            if !seen.insert(file_id) {
                continue;
            }
            let Some(meta) = self.snapshot.file_table.get(file_id) else {
                continue;
            };
            if meta.path_len == 0 || meta.name_len == 0 {
                continue;
            }
            let Some(path) = snapshot_path_for_id(&self.snapshot, file_id) else {
                continue;
            };
            if let Some((scope, scope_child_prefix)) = scope.as_ref() {
                if !path_is_in_normalized_scope(path, scope, scope_child_prefix) {
                    continue;
                }
            }

            ids.push(file_id);
        }

        Some(ids)
    }

    fn filter_file_ids_in_scope(&self, file_ids: &[FileId], scope: &Path) -> Option<Vec<FileId>> {
        let (scope, scope_child_prefix) = normalized_scope_parts(scope)?;
        Some(
            file_ids
                .iter()
                .copied()
                .filter(|&file_id| {
                    snapshot_path_for_id(&self.snapshot, file_id).is_some_and(|path| {
                        path_is_in_normalized_scope(path, &scope, &scope_child_prefix)
                    })
                })
                .collect(),
        )
    }

    fn exact_name_file_ids(&self, query: &str) -> Option<Vec<FileId>> {
        if !is_exact_basename_query(query) {
            return None;
        }
        let key = query.to_lowercase();
        self.name_to_ids.get(&key).cloned()
    }

    fn insert_name_mapping(&mut self, file_id: FileId) {
        let Some(meta) = self.snapshot.file_table.get(file_id) else {
            return;
        };
        let Some(name) = self
            .snapshot
            .string_arena
            .get(meta.name_offset, meta.name_len)
        else {
            return;
        };
        if name.is_empty() {
            return;
        }
        self.name_to_ids
            .entry(name.to_lowercase())
            .or_default()
            .push(file_id);
    }

    fn remove_name_mapping(&mut self, file_id: FileId, name: &str) {
        let key = name.to_lowercase();
        let Some(ids) = self.name_to_ids.get_mut(&key) else {
            return;
        };
        ids.retain(|&id| id != file_id);
        if ids.is_empty() {
            self.name_to_ids.remove(&key);
        }
    }

    fn mark_recent_update(&mut self, file_id: FileId) {
        let Some(meta) = self.snapshot.file_table.get(file_id) else {
            return;
        };
        if meta.path_len == 0 || meta.name_len == 0 {
            return;
        }
        self.recent_updates.retain(|&id| id != file_id);
        self.recent_updates.push(file_id);
        if self.recent_updates.len() > RECENT_UPDATE_LIMIT {
            let excess = self.recent_updates.len() - RECENT_UPDATE_LIMIT;
            self.recent_updates.drain(0..excess);
        }
    }

    fn remove_recent_update(&mut self, file_id: FileId) {
        self.recent_updates.retain(|&id| id != file_id);
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

    fn upsert_prepared(&mut self, file: PreparedFileMeta) {
        let path_str = file.path.as_str();
        let name_str = file.name.as_str();
        let inode_key = (file.dev, file.ino);

        if let Some(file_id) = self.get_file_id_for_path(path_str) {
            let (old_inode_key, old_name) = {
                let Some(meta) = self.snapshot.file_table.get(file_id) else {
                    return;
                };
                let old_name = self
                    .snapshot
                    .string_arena
                    .get(meta.name_offset, meta.name_len)
                    .unwrap_or("")
                    .to_string();
                ((meta.dev, meta.ino), old_name)
            };

            if old_name != name_str {
                self.remove_name_mapping(file_id, &old_name);
            }

            let Some(meta) = self.snapshot.file_table.get_mut(file_id) else {
                return;
            };

            if old_inode_key != inode_key {
                if self.inode_to_id.get(&old_inode_key) == Some(&file_id) {
                    self.inode_to_id.remove(&old_inode_key);
                }
                self.inode_to_id.insert(inode_key, file_id);
            }

            if old_name != name_str {
                self.snapshot.trigram_index.remove_text(file_id, &old_name);
                self.snapshot.trigram_index.add(file_id, name_str);

                let (name_offset, name_len) = self.snapshot.string_arena.add(name_str);
                meta.name_offset = name_offset;
                meta.name_len = name_len;
            }

            meta.size = file.size;
            meta.mtime = file.mtime;
            meta.dev = file.dev;
            meta.ino = file.ino;

            if old_name != name_str {
                self.insert_name_mapping(file_id);
            }
            self.mark_recent_update(file_id);
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
            if old_name != name_str {
                self.remove_name_mapping(file_id, &old_name);
            }

            let Some(meta) = self.snapshot.file_table.get_mut(file_id) else {
                return;
            };

            if old_name != name_str {
                self.snapshot.trigram_index.remove_text(file_id, &old_name);
                self.snapshot.trigram_index.add(file_id, name_str);

                let (name_offset, name_len) = self.snapshot.string_arena.add(name_str);
                meta.name_offset = name_offset;
                meta.name_len = name_len;
            }

            let (path_offset, path_len) = self.snapshot.string_arena.add(path_str);
            meta.path_offset = path_offset;
            meta.path_len = path_len;
            meta.size = file.size;
            meta.mtime = file.mtime;
            meta.dev = file.dev;
            meta.ino = file.ino;

            self.insert_path_mapping(path_str, file_id);
            self.mark_path_order_dirty();
            if old_name != name_str {
                self.insert_name_mapping(file_id);
            }
            self.mark_recent_update(file_id);
        } else {
            let (path_offset, path_len) = self.snapshot.string_arena.add(path_str);
            let (name_offset, name_len) = self.snapshot.string_arena.add(name_str);

            let new_meta = FileMeta {
                path_offset,
                path_len,
                name_offset,
                name_len,
                size: file.size,
                mtime: file.mtime,
                dev: file.dev,
                ino: file.ino,
            };

            let file_id = self.snapshot.file_table.insert(new_meta);
            self.snapshot.trigram_index.add(file_id, name_str);
            self.insert_path_mapping(path_str, file_id);
            self.mark_path_order_dirty();
            self.insert_name_mapping(file_id);
            self.mark_recent_update(file_id);
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
        let (inode_key, old_name) = {
            let Some(meta) = self.snapshot.file_table.get(file_id) else {
                return;
            };
            let old_name = self
                .snapshot
                .string_arena
                .get(meta.name_offset, meta.name_len)
                .unwrap_or("")
                .to_string();
            ((meta.dev, meta.ino), old_name)
        };

        if inode_key != (0, 0) && self.inode_to_id.get(&inode_key) == Some(&file_id) {
            self.inode_to_id.remove(&inode_key);
        }

        self.mark_path_order_dirty();
        self.remove_recent_update(file_id);
        self.snapshot.trigram_index.remove_text(file_id, &old_name);
        self.remove_name_mapping(file_id, &old_name);

        let Some(meta) = self.snapshot.file_table.get_mut(file_id) else {
            return;
        };

        // Tombstone the entry (keeps IDs stable).
        meta.path_len = 0;
        meta.name_len = 0;
        meta.size = 0;
        meta.mtime = 0;

        self.last_updated = now_epoch_seconds();
    }

    fn move_prepared(&mut self, from: &Path, file: Option<PreparedFileMeta>) {
        let from_str = from.to_string_lossy();
        let Some(file_id) = self.remove_path_mapping(from_str.as_ref()) else {
            // If we didn't know about the old path, treat as a create on the new path.
            if let Some(file) = file {
                self.upsert_prepared(file);
            }
            return;
        };

        let Some(file) = file else {
            self.tombstone_file(file_id);
            return;
        };

        let to_str = file.path.as_str();
        let name_str = file.name.as_str();

        if let Some(overwritten_id) = self
            .get_file_id_for_path(to_str)
            .filter(|&existing_id| existing_id != file_id)
        {
            let _ = self.remove_path_mapping(to_str);
            self.remove_recent_update(overwritten_id);
            self.tombstone_file(overwritten_id);
        }

        let (old_inode_key, old_name) = {
            let Some(meta) = self.snapshot.file_table.get(file_id) else {
                return;
            };
            let old_name = self
                .snapshot
                .string_arena
                .get(meta.name_offset, meta.name_len)
                .unwrap_or("")
                .to_string();
            ((meta.dev, meta.ino), old_name)
        };

        if old_name != name_str {
            self.remove_name_mapping(file_id, &old_name);
            self.snapshot.trigram_index.remove_text(file_id, &old_name);
            self.snapshot.trigram_index.add(file_id, name_str);
        }

        let Some(meta) = self.snapshot.file_table.get_mut(file_id) else {
            return;
        };

        if old_name != name_str {
            let (name_offset, name_len) = self.snapshot.string_arena.add(name_str);
            meta.name_offset = name_offset;
            meta.name_len = name_len;
        }

        let (path_offset, path_len) = self.snapshot.string_arena.add(to_str);

        meta.path_offset = path_offset;
        meta.path_len = path_len;
        meta.size = file.size;
        meta.mtime = file.mtime;
        meta.dev = file.dev;
        meta.ino = file.ino;

        let new_inode_key = (file.dev, file.ino);
        if old_inode_key != new_inode_key {
            if self.inode_to_id.get(&old_inode_key) == Some(&file_id) {
                self.inode_to_id.remove(&old_inode_key);
            }
            self.inode_to_id.insert(new_inode_key, file_id);
        } else {
            self.inode_to_id.insert(new_inode_key, file_id);
        }

        self.insert_path_mapping(to_str, file_id);
        self.mark_path_order_dirty();
        if old_name != name_str {
            self.insert_name_mapping(file_id);
        }
        self.mark_recent_update(file_id);
        self.last_updated = now_epoch_seconds();
    }
}

fn normalized_scope_parts(scope: &Path) -> Option<(String, String)> {
    let scope = scope.to_str()?.trim_end_matches('/');
    let scope = if scope.is_empty() {
        "/".to_string()
    } else {
        scope.to_string()
    };
    let scope_child_prefix = if scope == "/" {
        "/".to_string()
    } else {
        format!("{scope}/")
    };
    Some((scope, scope_child_prefix))
}

fn path_is_in_normalized_scope(path: &str, scope: &str, scope_child_prefix: &str) -> bool {
    path == scope || path.starts_with(scope_child_prefix)
}

fn is_exact_basename_query(query: &str) -> bool {
    !query.is_empty()
        && query.bytes().any(|b| matches!(b, b'.' | b'-' | b'_'))
        && !query.bytes().any(|b| matches!(b, b'/' | b'\\'))
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

fn build_path_order(snapshot: &IndexSnapshot) -> Vec<FileId> {
    let mut ids: Vec<FileId> = snapshot
        .file_table
        .iter()
        .filter_map(|(file_id, meta)| (meta.path_len > 0).then_some(file_id))
        .collect();
    ids.sort_unstable_by(|&a, &b| {
        let a_path = snapshot_path_for_id(snapshot, a).unwrap_or("");
        let b_path = snapshot_path_for_id(snapshot, b).unwrap_or("");
        a_path.cmp(b_path).then_with(|| a.cmp(&b))
    });
    ids
}

fn build_name_map(snapshot: &IndexSnapshot) -> std::collections::HashMap<String, Vec<FileId>> {
    let mut map = std::collections::HashMap::<String, Vec<FileId>>::new();
    for (file_id, meta) in snapshot.file_table.iter() {
        if meta.name_len == 0 || meta.path_len == 0 {
            continue;
        }
        let Some(name) = snapshot.string_arena.get(meta.name_offset, meta.name_len) else {
            continue;
        };
        map.entry(name.to_lowercase()).or_default().push(file_id);
    }
    map
}

fn build_recent_order(snapshot: &IndexSnapshot) -> Vec<FileId> {
    let mut ids: Vec<FileId> = snapshot
        .file_table
        .iter()
        .filter_map(|(file_id, meta)| (meta.path_len > 0 && meta.name_len > 0).then_some(file_id))
        .collect();
    ids.sort_unstable_by(|&a, &b| {
        let a_meta = snapshot.file_table.get(a);
        let b_meta = snapshot.file_table.get(b);
        let a_mtime = a_meta.map(|m| m.mtime).unwrap_or_default();
        let b_mtime = b_meta.map(|m| m.mtime).unwrap_or_default();
        b_mtime.cmp(&a_mtime).then_with(|| a.cmp(&b))
    });
    ids
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

fn replace_state(state: &SharedState, rebuilt: DaemonState) {
    let old_state = {
        let mut state = state.write().unwrap();
        std::mem::replace(&mut *state, rebuilt)
    };

    // Dropping a multi-GB old state can monopolize allocator locks and stall
    // foreground IPC threads even though the daemon state lock has been
    // released. Rebuilds are rare; keep the retired state until process exit so
    // status/search stay responsive through reconcile completion.
    std::mem::forget(old_state);
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
        let scanner = Scanner::new(config.clone());
        let snapshot = scanner.scan()?;
        let files_indexed = snapshot.file_table.len();

        // Finalize without holding the shared state write lock for expensive work.
        // Holding the journal lock blocks watcher writes, but search/status can keep
        // reading the previous hot snapshot until the final pointer-sized swap.
        let _journal_guard = journal_lock.lock().unwrap();
        let applied_updates = {
            let mut rebuilt =
                DaemonState::new(config, index_file.clone(), journal_file.clone(), snapshot);
            let applied_updates = apply_journal_from_offset(&journal_file, journal_offset, |u| {
                rebuilt.apply_update(u);
            });
            if applied_updates > 0 {
                debug!("Applied {} journal updates after rebuild", applied_updates);
            }

            rebuilt.snapshot.save(&index_file)?;
            truncate_journal(&journal_file)?;
            rebuilt.last_updated = now_epoch_seconds();
            rebuilt.reconciling = false;

            replace_state(state, rebuilt);
            applied_updates
        };

        info!("Full rebuild complete: {} files indexed", files_indexed);
        if applied_updates > 0 {
            debug!("Full rebuild included {} journal updates", applied_updates);
        }

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
    handler: IpcHandler,
    socket_path: PathBuf,
}

#[derive(Clone)]
struct IpcHandler {
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
            socket_path: socket_path.to_path_buf(),
            handler: IpcHandler {
                state,
                shutdown,
                socket_path: socket_path.to_path_buf(),
                journal_lock,
                rebuild_lock,
            },
        })
    }

    /// Run the server loop.
    pub fn run(&self) -> Result<()> {
        while !self.handler.shutdown.load(Ordering::Relaxed) {
            match self.listener.accept() {
                Ok((stream, _addr)) => {
                    if let Err(e) = stream.set_nonblocking(false) {
                        error!("Failed to set client stream blocking mode: {}", e);
                    }
                    let handler = self.handler.clone();
                    std::thread::spawn(move || handler.handle_client(stream));
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        }

        Ok(())
    }
}

impl IpcHandler {
    /// Handle a single client connection.
    fn handle_client(&self, mut stream: UnixStream) {
        let peer_addr = stream.peer_addr().ok();
        debug!("Client connected: {:?}", peer_addr);

        let mut reader = BufReader::new(stream.try_clone().unwrap());

        loop {
            match vicaya_core::ipc::read_message(&mut reader) {
                Ok(None) => {
                    debug!("Client disconnected");
                    return;
                }
                Ok(Some(line)) => {
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

                    if self.shutdown.load(Ordering::Relaxed) {
                        return;
                    }
                }
                Err(e) => {
                    error!("Failed to read from client: {}", e);
                    let response = Response::Error {
                        message: e.to_string(),
                    };
                    self.send_response(&mut stream, &response);
                    return;
                }
            }
        }
    }

    /// Handle a request and generate a response.
    fn handle_request(&self, request: Request) -> Response {
        match request {
            Request::Search {
                query,
                limit,
                scope,
                filter_scope,
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
                let filter_scope_path = filter_scope
                    .filter(|s| !s.trim().is_empty())
                    .map(std::path::PathBuf::from);
                const SCOPED_LINEAR_SEARCH_LIMIT: usize = 100_000;
                let scoped_file_ids = filter_scope_path.as_deref().and_then(|scope| {
                    state.scoped_file_ids_up_to(scope, SCOPED_LINEAR_SEARCH_LIMIT)
                });
                let exact_name_file_ids = state.exact_name_file_ids(&query).map(|ids| {
                    if let Some(scope) = filter_scope_path.as_deref() {
                        state
                            .filter_file_ids_in_scope(&ids, scope)
                            .unwrap_or_default()
                    } else {
                        ids
                    }
                });

                // If query is empty and recent_if_empty is true, return recent files
                let results = if query.trim().is_empty() && recent_if_empty {
                    if let Some((file_ids, true)) = scoped_file_ids.as_ref() {
                        engine.recent_file_ids(limit, file_ids)
                    } else {
                        let file_ids = state
                            .recent_file_ids(limit, filter_scope_path.as_deref())
                            .unwrap_or_default();
                        engine.recent_file_ids(limit, &file_ids)
                    }
                } else if let Some(file_ids) = exact_name_file_ids.as_deref() {
                    engine.exact_name_file_ids(limit, file_ids)
                } else if let Some((file_ids, true)) = scoped_file_ids.as_ref() {
                    let query_obj = Query {
                        term: query,
                        limit,
                        scope: scope_path,
                        filter_scope: filter_scope_path,
                    };
                    engine.search_file_ids(&query_obj, file_ids)
                } else {
                    let query_obj = Query {
                        term: query,
                        limit,
                        scope: scope_path,
                        filter_scope: filter_scope_path,
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
                let _ = UnixStream::connect(&self.socket_path);
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

impl IpcServer {
    #[cfg(test)]
    fn handle_request(&self, request: Request) -> Response {
        self.handler.handle_request(request)
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use vicaya_core::config::PerformanceConfig;
    use vicaya_scanner::Scanner;

    fn test_config(root: &Path, vicaya_dir: &Path) -> Config {
        Config {
            index_roots: vec![root.to_path_buf()],
            exclusions: vec![],
            respect_ignore_files: true,
            index_path: vicaya_dir.join("index"),
            max_memory_mb: 128,
            performance: PerformanceConfig {
                scanner_threads: 2,
                reconcile_hour: 3,
            },
        }
    }

    fn build_state(root: &Path, vicaya_dir: &Path) -> DaemonState {
        let config = test_config(root, vicaya_dir);
        std::fs::create_dir_all(&config.index_path).unwrap();
        let snapshot = Scanner::new(config.clone()).scan().unwrap();
        DaemonState::new(
            config,
            vicaya_dir.join("index.bin"),
            vicaya_dir.join("journal.log"),
            snapshot,
        )
    }

    fn inode_key_for(state: &DaemonState, file_id: FileId) -> (u64, u64) {
        let meta = state.snapshot.file_table.get(file_id).unwrap();
        (meta.dev, meta.ino)
    }

    #[test]
    fn move_path_updates_maps_for_plain_rename() {
        let vicaya_dir = tempdir().unwrap();
        let root = tempdir().unwrap();

        let from = root.path().join("from.txt");
        let to = root.path().join("renamed.txt");
        std::fs::write(&from, "from").unwrap();

        let mut state = build_state(root.path(), vicaya_dir.path());
        let file_id = state.get_file_id_for_path(&from.to_string_lossy()).unwrap();

        std::fs::rename(&from, &to).unwrap();
        state.apply_update(IndexUpdate::Move {
            from: from.to_string_lossy().to_string(),
            to: to.to_string_lossy().to_string(),
        });

        assert!(state
            .get_file_id_for_path(&from.to_string_lossy())
            .is_none());
        assert_eq!(
            state.get_file_id_for_path(&to.to_string_lossy()),
            Some(file_id)
        );
        assert_eq!(
            snapshot_path_for_id(&state.snapshot, file_id),
            Some(to.to_string_lossy().as_ref())
        );
        assert_eq!(
            state.inode_to_id.get(&inode_key_for(&state, file_id)),
            Some(&file_id)
        );
    }

    #[test]
    fn move_path_tombstones_overwritten_destination_and_clears_inode_mapping() {
        let vicaya_dir = tempdir().unwrap();
        let root = tempdir().unwrap();

        let from = root.path().join("from.txt");
        let to = root.path().join("to.txt");
        std::fs::write(&from, "from").unwrap();
        std::fs::write(&to, "to").unwrap();

        let mut state = build_state(root.path(), vicaya_dir.path());
        let from_id = state.get_file_id_for_path(&from.to_string_lossy()).unwrap();
        let overwritten_id = state.get_file_id_for_path(&to.to_string_lossy()).unwrap();
        let overwritten_inode = inode_key_for(&state, overwritten_id);

        std::fs::rename(&from, &to).unwrap();
        state.apply_update(IndexUpdate::Move {
            from: from.to_string_lossy().to_string(),
            to: to.to_string_lossy().to_string(),
        });

        assert!(state
            .get_file_id_for_path(&from.to_string_lossy())
            .is_none());
        assert_eq!(
            state.get_file_id_for_path(&to.to_string_lossy()),
            Some(from_id)
        );
        assert_eq!(state.inode_to_id.get(&overwritten_inode), None);

        let tombstoned = state.snapshot.file_table.get(overwritten_id).unwrap();
        assert_eq!(tombstoned.path_len, 0);
        assert_eq!(tombstoned.name_len, 0);
        assert!(
            !state.inode_to_id.values().any(|&id| id == overwritten_id),
            "overwritten destination should not survive in inode map"
        );
    }

    #[test]
    fn apply_update_create_modify_delete_and_exclusions_keep_maps_consistent() {
        let vicaya_dir = tempdir().unwrap();
        let root = tempdir().unwrap();
        let mut state = build_state(root.path(), vicaya_dir.path());

        let file = root.path().join("note.txt");
        std::fs::write(&file, "one").unwrap();
        state.apply_update(IndexUpdate::Create {
            path: file.to_string_lossy().to_string(),
        });
        let file_id = state.get_file_id_for_path(&file.to_string_lossy()).unwrap();
        assert!(state.indexed_file_count() >= 1);

        std::fs::write(&file, "updated").unwrap();
        state.apply_update(IndexUpdate::Modify {
            path: file.to_string_lossy().to_string(),
        });
        let meta = state.snapshot.file_table.get(file_id).unwrap();
        assert_eq!(meta.size, 7);

        state.config.exclusions.push("target".to_string());
        let excluded = root.path().join("target").join("ignored.txt");
        std::fs::create_dir_all(excluded.parent().unwrap()).unwrap();
        std::fs::write(&excluded, "ignored").unwrap();
        state.apply_update(IndexUpdate::Create {
            path: excluded.to_string_lossy().to_string(),
        });
        assert!(state
            .get_file_id_for_path(&excluded.to_string_lossy())
            .is_none());

        state.apply_update(IndexUpdate::Delete {
            path: file.to_string_lossy().to_string(),
        });
        assert!(state
            .get_file_id_for_path(&file.to_string_lossy())
            .is_none());
        assert_eq!(state.snapshot.file_table.get(file_id).unwrap().path_len, 0);
    }

    #[test]
    fn move_unknown_source_upserts_destination_and_excluded_move_tombstones() {
        let vicaya_dir = tempdir().unwrap();
        let root = tempdir().unwrap();
        let mut state = build_state(root.path(), vicaya_dir.path());

        let unknown = root.path().join("missing.txt");
        let created = root.path().join("created.txt");
        std::fs::write(&created, "created").unwrap();
        state.apply_update(IndexUpdate::Move {
            from: unknown.to_string_lossy().to_string(),
            to: created.to_string_lossy().to_string(),
        });
        let created_id = state
            .get_file_id_for_path(&created.to_string_lossy())
            .unwrap();

        state.config.exclusions.push("target".to_string());
        let excluded = root.path().join("target").join("created.txt");
        std::fs::create_dir_all(excluded.parent().unwrap()).unwrap();
        std::fs::rename(&created, &excluded).unwrap();
        state.apply_update(IndexUpdate::Move {
            from: created.to_string_lossy().to_string(),
            to: excluded.to_string_lossy().to_string(),
        });

        assert!(state
            .get_file_id_for_path(&created.to_string_lossy())
            .is_none());
        assert!(state
            .get_file_id_for_path(&excluded.to_string_lossy())
            .is_none());
        assert_eq!(
            state.snapshot.file_table.get(created_id).unwrap().path_len,
            0
        );
    }

    #[test]
    fn journal_replay_skips_bad_lines_and_truncate_resets_file() {
        let dir = tempdir().unwrap();
        let journal = dir.path().join("index.journal");
        let first = IndexUpdate::Create {
            path: "/tmp/one.txt".to_string(),
        };
        let second = IndexUpdate::Delete {
            path: "/tmp/two.txt".to_string(),
        };
        let first_line = serde_json::to_string(&first).unwrap();
        let offset = first_line.len() as u64 + 1;
        std::fs::write(
            &journal,
            format!(
                "{first_line}\nnot-json\n{}\n\n",
                serde_json::to_string(&second).unwrap()
            ),
        )
        .unwrap();

        let mut applied = Vec::new();
        let count = apply_journal_from_offset(&journal, offset, |update| applied.push(update));
        assert_eq!(count, 1);
        assert!(matches!(applied[0], IndexUpdate::Delete { .. }));

        truncate_journal(&journal).unwrap();
        assert_eq!(std::fs::metadata(&journal).unwrap().len(), 0);
    }

    #[test]
    fn full_rebuild_refreshes_last_updated_after_swap() {
        let vicaya_dir = tempdir().unwrap();
        let root = tempdir().unwrap();
        std::fs::write(root.path().join("Cargo.toml"), "[package]\n").unwrap();

        let state = Arc::new(RwLock::new(build_state(root.path(), vicaya_dir.path())));
        state.write().unwrap().last_updated = 0;

        let files_indexed =
            full_rebuild_from_disk(&state, &Arc::new(Mutex::new(())), &Arc::new(Mutex::new(())))
                .unwrap();

        assert!(files_indexed >= 1);
        let state = state.read().unwrap();
        assert!(state.last_updated > 0);
        assert!(!state.reconciling);
    }

    #[test]
    fn ipc_server_handle_request_covers_status_search_rebuild_and_shutdown() {
        let vicaya_dir = tempdir().unwrap();
        let root = tempdir().unwrap();
        let cargo = root.path().join("Cargo.toml");
        std::fs::write(&cargo, "[package]\n").unwrap();

        let state = Arc::new(RwLock::new(build_state(root.path(), vicaya_dir.path())));
        let shutdown = Arc::new(AtomicBool::new(false));
        let journal_lock = Arc::new(Mutex::new(()));
        let rebuild_lock = Arc::new(Mutex::new(()));
        let socket = vicaya_dir.path().join("daemon.sock");
        let server =
            IpcServer::new(&socket, state, shutdown.clone(), journal_lock, rebuild_lock).unwrap();

        match server.handle_request(Request::Status) {
            Response::Status {
                indexed_files,
                trigram_count,
                ..
            } => {
                assert!(indexed_files >= 1);
                assert!(trigram_count > 0);
            }
            other => panic!("unexpected status response: {other:?}"),
        }

        match server.handle_request(Request::Search {
            query: "Cargo".to_string(),
            limit: 10,
            scope: Some(root.path().to_string_lossy().to_string()),
            filter_scope: Some(root.path().to_string_lossy().to_string()),
            recent_if_empty: false,
        }) {
            Response::SearchResults { results } => {
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].path, cargo.to_string_lossy());
            }
            other => panic!("unexpected search response: {other:?}"),
        }

        match server.handle_request(Request::Search {
            query: String::new(),
            limit: 10,
            scope: None,
            filter_scope: Some(root.path().to_string_lossy().to_string()),
            recent_if_empty: true,
        }) {
            Response::SearchResults { results } => {
                assert!(results.iter().any(|r| r.path == cargo.to_string_lossy()))
            }
            other => panic!("unexpected recent response: {other:?}"),
        }

        match server.handle_request(Request::Rebuild { dry_run: true }) {
            Response::RebuildComplete { files_indexed } => assert!(files_indexed >= 1),
            other => panic!("unexpected rebuild response: {other:?}"),
        }

        assert!(matches!(
            server.handle_request(Request::Shutdown),
            Response::Ok
        ));
        assert!(shutdown.load(Ordering::Relaxed));
    }

    #[test]
    fn scoped_file_ids_up_to_reports_incomplete_large_scope() {
        let vicaya_dir = tempdir().unwrap();
        let root = tempdir().unwrap();
        std::fs::write(root.path().join("a.txt"), "a").unwrap();
        std::fs::write(root.path().join("b.txt"), "b").unwrap();
        std::fs::write(root.path().join("c.txt"), "c").unwrap();

        let state = build_state(root.path(), vicaya_dir.path());
        let (ids, complete) = state.scoped_file_ids_up_to(root.path(), 1).unwrap();

        assert!(!complete);
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn scoped_file_id_cache_is_disabled_after_incremental_path_changes() {
        let vicaya_dir = tempdir().unwrap();
        let root = tempdir().unwrap();
        let mut state = build_state(root.path(), vicaya_dir.path());

        let new_file = root.path().join("new.txt");
        std::fs::write(&new_file, "new").unwrap();
        state.apply_update(IndexUpdate::Create {
            path: new_file.to_string_lossy().to_string(),
        });

        assert!(state.path_order_dirty);
        assert!(state.scoped_file_ids_up_to(root.path(), 10).is_none());
    }

    #[test]
    fn exact_name_search_is_filtered_inside_scope() {
        let vicaya_dir = tempdir().unwrap();
        let root = tempdir().unwrap();
        let inside_dir = root.path().join("inside");
        let outside_dir = root.path().join("outside");
        std::fs::create_dir_all(&inside_dir).unwrap();
        std::fs::create_dir_all(&outside_dir).unwrap();
        let inside = inside_dir.join("main.go");
        let outside = outside_dir.join("main.go");
        std::fs::write(&inside, "package main\n").unwrap();
        std::fs::write(&outside, "package main\n").unwrap();

        let state = Arc::new(RwLock::new(build_state(root.path(), vicaya_dir.path())));
        let shutdown = Arc::new(AtomicBool::new(false));
        let journal_lock = Arc::new(Mutex::new(()));
        let rebuild_lock = Arc::new(Mutex::new(()));
        let socket = vicaya_dir.path().join("daemon.sock");
        let server = IpcServer::new(&socket, state, shutdown, journal_lock, rebuild_lock).unwrap();

        match server.handle_request(Request::Search {
            query: "main.go".to_string(),
            limit: 10,
            scope: Some(inside_dir.to_string_lossy().to_string()),
            filter_scope: Some(inside_dir.to_string_lossy().to_string()),
            recent_if_empty: false,
        }) {
            Response::SearchResults { results } => {
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].path, inside.to_string_lossy());
            }
            other => panic!("unexpected exact scoped search response: {other:?}"),
        }

        assert!(outside.exists());
    }

    #[test]
    fn ipc_server_accepts_persistent_client_requests() {
        use std::io::Write as _;
        use std::os::unix::net::UnixStream;

        let vicaya_dir = tempdir().unwrap();
        let root = tempdir().unwrap();
        let cargo = root.path().join("Cargo.toml");
        std::fs::write(&cargo, "[package]\n").unwrap();

        let state = Arc::new(RwLock::new(build_state(root.path(), vicaya_dir.path())));
        let shutdown = Arc::new(AtomicBool::new(false));
        let journal_lock = Arc::new(Mutex::new(()));
        let rebuild_lock = Arc::new(Mutex::new(()));
        let socket = vicaya_dir.path().join("daemon.sock");
        let server =
            IpcServer::new(&socket, state, shutdown.clone(), journal_lock, rebuild_lock).unwrap();
        let server_thread = std::thread::spawn(move || server.run().unwrap());

        let mut stream = UnixStream::connect(&socket).unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());

        let send = |stream: &mut UnixStream, request: Request| {
            let mut json = request.to_json().unwrap();
            json.push('\n');
            stream.write_all(json.as_bytes()).unwrap();
        };

        send(&mut stream, Request::Status);
        let line = vicaya_core::ipc::read_message(&mut reader)
            .unwrap()
            .unwrap();
        assert!(matches!(
            Response::from_json(&line).unwrap(),
            Response::Status { .. }
        ));

        send(
            &mut stream,
            Request::Search {
                query: "Cargo".to_string(),
                limit: 10,
                scope: Some(root.path().to_string_lossy().to_string()),
                filter_scope: Some(root.path().to_string_lossy().to_string()),
                recent_if_empty: false,
            },
        );
        let line = vicaya_core::ipc::read_message(&mut reader)
            .unwrap()
            .unwrap();
        match Response::from_json(&line).unwrap() {
            Response::SearchResults { results } => assert_eq!(results.len(), 1),
            other => panic!("unexpected persistent search response: {other:?}"),
        }

        send(&mut stream, Request::Shutdown);
        let line = vicaya_core::ipc::read_message(&mut reader)
            .unwrap()
            .unwrap();
        assert!(matches!(Response::from_json(&line).unwrap(), Response::Ok));

        drop(reader);
        drop(stream);
        server_thread.join().unwrap();
        assert!(shutdown.load(Ordering::Relaxed));
    }

    #[test]
    fn ipc_server_handles_multiple_tui_clients_concurrently() {
        use std::io::Write as _;
        use std::os::unix::net::UnixStream;

        let vicaya_dir = tempdir().unwrap();
        let root = tempdir().unwrap();
        let cargo = root.path().join("Cargo.toml");
        std::fs::write(&cargo, "[package]\n").unwrap();

        let state = Arc::new(RwLock::new(build_state(root.path(), vicaya_dir.path())));
        let shutdown = Arc::new(AtomicBool::new(false));
        let journal_lock = Arc::new(Mutex::new(()));
        let rebuild_lock = Arc::new(Mutex::new(()));
        let socket = vicaya_dir.path().join("daemon.sock");
        let server =
            IpcServer::new(&socket, state, shutdown.clone(), journal_lock, rebuild_lock).unwrap();
        let server_thread = std::thread::spawn(move || server.run().unwrap());

        let mut clients = Vec::new();
        for _ in 0..4 {
            let socket = socket.clone();
            let scope = root.path().to_string_lossy().to_string();
            clients.push(std::thread::spawn(move || {
                let mut stream = UnixStream::connect(&socket).unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());

                let send = |stream: &mut UnixStream, request: Request| {
                    let mut json = request.to_json().unwrap();
                    json.push('\n');
                    stream.write_all(json.as_bytes()).unwrap();
                };

                send(&mut stream, Request::Status);
                let line = vicaya_core::ipc::read_message(&mut reader)
                    .unwrap()
                    .unwrap();
                assert!(matches!(
                    Response::from_json(&line).unwrap(),
                    Response::Status { .. }
                ));

                send(
                    &mut stream,
                    Request::Search {
                        query: "Cargo".to_string(),
                        limit: 10,
                        scope: Some(scope.clone()),
                        filter_scope: Some(scope),
                        recent_if_empty: false,
                    },
                );
                let line = vicaya_core::ipc::read_message(&mut reader)
                    .unwrap()
                    .unwrap();
                match Response::from_json(&line).unwrap() {
                    Response::SearchResults { results } => assert_eq!(results.len(), 1),
                    other => panic!("unexpected concurrent search response: {other:?}"),
                }
            }));
        }

        for client in clients {
            client.join().unwrap();
        }

        let mut stream = UnixStream::connect(&socket).unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut json = Request::Shutdown.to_json().unwrap();
        json.push('\n');
        stream.write_all(json.as_bytes()).unwrap();
        let line = vicaya_core::ipc::read_message(&mut reader)
            .unwrap()
            .unwrap();
        assert!(matches!(Response::from_json(&line).unwrap(), Response::Ok));

        server_thread.join().unwrap();
        assert!(shutdown.load(Ordering::Relaxed));
    }
}
