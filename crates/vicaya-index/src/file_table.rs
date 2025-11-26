//! File table and metadata types.

use serde::{Deserialize, Serialize};

/// Unique identifier for a file entry.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileId(pub u64);

/// Metadata for a single file entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMeta {
    /// Offset into the string arena for the full path.
    pub path_offset: usize,
    /// Length of the full path string.
    pub path_len: usize,
    /// Offset into the string arena for just the basename.
    pub name_offset: usize,
    /// Length of the basename string.
    pub name_len: usize,
    /// File size in bytes.
    pub size: u64,
    /// Modification time (Unix timestamp).
    pub mtime: i64,
    /// Device ID.
    pub dev: u64,
    /// Inode number.
    pub ino: u64,
}

/// File table: collection of all indexed files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTable {
    entries: Vec<FileMeta>,
}

impl FileTable {
    /// Create a new empty file table.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Insert a new file entry.
    pub fn insert(&mut self, meta: FileMeta) -> FileId {
        let id = FileId(self.entries.len() as u64);
        self.entries.push(meta);
        id
    }

    /// Get a file entry by ID.
    pub fn get(&self, id: FileId) -> Option<&FileMeta> {
        self.entries.get(id.0 as usize)
    }

    /// Get a mutable reference to a file entry by ID.
    pub fn get_mut(&mut self, id: FileId) -> Option<&mut FileMeta> {
        self.entries.get_mut(id.0 as usize)
    }

    /// Iterate over all entries.
    pub fn iter(&self) -> impl Iterator<Item = (FileId, &FileMeta)> {
        self.entries
            .iter()
            .enumerate()
            .map(|(i, meta)| (FileId(i as u64), meta))
    }

    /// Number of entries in the table.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the table is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for FileTable {
    fn default() -> Self {
        Self::new()
    }
}
