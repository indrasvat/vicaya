//! File table and metadata types.

use serde::{Deserialize, Serialize};

/// Unique identifier for a file entry.
///
/// Uses u32 to support up to 4.2 billion files while minimizing memory usage.
/// This is more than sufficient for any single-machine filesystem indexer.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileId(pub u32);

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
    ///
    /// # Panics
    /// Panics if the file table exceeds u32::MAX entries (4.2 billion files).
    pub fn insert(&mut self, meta: FileMeta) -> FileId {
        let id = self.entries.len();
        assert!(
            id <= u32::MAX as usize,
            "File table exceeded u32::MAX capacity ({} files)",
            u32::MAX
        );
        let file_id = FileId(id as u32);
        self.entries.push(meta);
        file_id
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
            .map(|(i, meta)| (FileId(i as u32), meta))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_meta(path_offset: usize, name_offset: usize) -> FileMeta {
        FileMeta {
            path_offset,
            path_len: 10,
            name_offset,
            name_len: 5,
            size: 1024,
            mtime: 1234567890,
            dev: 1,
            ino: 100,
        }
    }

    #[test]
    fn test_new_table_is_empty() {
        let table = FileTable::new();
        assert!(table.is_empty());
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn test_insert_and_get() {
        let mut table = FileTable::new();
        let meta = create_test_meta(0, 10);

        let id = table.insert(meta);
        assert_eq!(id, FileId(0));
        assert_eq!(table.len(), 1);
        assert!(!table.is_empty());

        let retrieved = table.get(id).unwrap();
        assert_eq!(retrieved.path_offset, 0);
        assert_eq!(retrieved.name_offset, 10);
        assert_eq!(retrieved.size, 1024);
    }

    #[test]
    fn test_multiple_inserts() {
        let mut table = FileTable::new();

        let id1 = table.insert(create_test_meta(0, 10));
        let id2 = table.insert(create_test_meta(20, 30));
        let id3 = table.insert(create_test_meta(40, 50));

        assert_eq!(id1, FileId(0));
        assert_eq!(id2, FileId(1));
        assert_eq!(id3, FileId(2));
        assert_eq!(table.len(), 3);

        assert_eq!(table.get(id1).unwrap().path_offset, 0);
        assert_eq!(table.get(id2).unwrap().path_offset, 20);
        assert_eq!(table.get(id3).unwrap().path_offset, 40);
    }

    #[test]
    fn test_get_invalid_id() {
        let table = FileTable::new();
        assert!(table.get(FileId(0)).is_none());
        assert!(table.get(FileId(999)).is_none());
    }

    #[test]
    fn test_get_mut() {
        let mut table = FileTable::new();
        let id = table.insert(create_test_meta(0, 10));

        {
            let meta = table.get_mut(id).unwrap();
            meta.size = 2048;
        }

        assert_eq!(table.get(id).unwrap().size, 2048);
    }

    #[test]
    fn test_iter() {
        let mut table = FileTable::new();
        table.insert(create_test_meta(0, 10));
        table.insert(create_test_meta(20, 30));
        table.insert(create_test_meta(40, 50));

        let entries: Vec<_> = table.iter().collect();
        assert_eq!(entries.len(), 3);

        assert_eq!(entries[0].0, FileId(0));
        assert_eq!(entries[1].0, FileId(1));
        assert_eq!(entries[2].0, FileId(2));

        assert_eq!(entries[0].1.path_offset, 0);
        assert_eq!(entries[1].1.path_offset, 20);
        assert_eq!(entries[2].1.path_offset, 40);
    }

    #[test]
    fn test_default() {
        let table = FileTable::default();
        assert!(table.is_empty());
    }

    #[test]
    fn test_file_id_equality() {
        let id1 = FileId(42);
        let id2 = FileId(42);
        let id3 = FileId(100);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }
}
