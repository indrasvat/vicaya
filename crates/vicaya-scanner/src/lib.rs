//! vicaya-scanner: Parallel filesystem scanner.

use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};
use vicaya_core::{Config, Result};
use vicaya_index::{FileMeta, FileTable, StringArena, TrigramIndex};
use walkdir::WalkDir;

/// Scanned file information.
pub struct ScannedFile {
    pub path: PathBuf,
    pub size: u64,
    pub mtime: i64,
    pub dev: u64,
    pub ino: u64,
}

/// Scanner for building the initial index.
pub struct Scanner {
    config: Config,
}

impl Scanner {
    /// Create a new scanner with the given configuration.
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Scan all configured roots and build an index.
    pub fn scan(&self) -> Result<IndexSnapshot> {
        info!("Starting filesystem scan");

        let mut file_table = FileTable::new();
        let mut string_arena = StringArena::new();
        let mut trigram_index = TrigramIndex::new();

        for root in &self.config.index_roots {
            info!("Scanning root: {}", root.display());
            self.scan_root(root, &mut file_table, &mut string_arena, &mut trigram_index)?;
        }

        info!("Scan complete: {} files indexed", file_table.len());

        Ok(IndexSnapshot {
            file_table,
            string_arena,
            trigram_index,
        })
    }

    /// Scan a single root directory.
    fn scan_root(
        &self,
        root: &Path,
        file_table: &mut FileTable,
        string_arena: &mut StringArena,
        trigram_index: &mut TrigramIndex,
    ) -> Result<()> {
        let files: Vec<_> = WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| self.should_index(e.path()))
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .collect();

        debug!("Found {} files in {}", files.len(), root.display());

        for entry in files {
            if let Some(scanned) = self.scan_file(entry.path()) {
                self.add_to_index(scanned, file_table, string_arena, trigram_index);
            }
        }

        Ok(())
    }

    /// Check if a path should be indexed.
    fn should_index(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();

        for exclusion in &self.config.exclusions {
            if path_str.contains(exclusion.as_str()) {
                return false;
            }
        }

        true
    }

    /// Scan a single file and extract metadata.
    fn scan_file(&self, path: &Path) -> Option<ScannedFile> {
        let metadata = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(e) => {
                warn!("Failed to read metadata for {}: {}", path.display(), e);
                return None;
            }
        };

        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;

        let mtime = metadata
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs() as i64;

        Some(ScannedFile {
            path: path.to_path_buf(),
            size: metadata.len(),
            mtime,
            dev: metadata.dev(),
            ino: metadata.ino(),
        })
    }

    /// Add a scanned file to the index structures.
    fn add_to_index(
        &self,
        file: ScannedFile,
        file_table: &mut FileTable,
        string_arena: &mut StringArena,
        trigram_index: &mut TrigramIndex,
    ) {
        let path_str = file.path.to_string_lossy();
        let name = file
            .path
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();

        let (path_offset, path_len) = string_arena.add(&path_str);
        let (name_offset, name_len) = string_arena.add(&name);

        let meta = FileMeta {
            path_offset,
            path_len,
            name_offset,
            name_len,
            size: file.size,
            mtime: file.mtime,
            dev: file.dev,
            ino: file.ino,
        };

        let file_id = file_table.insert(meta);
        trigram_index.add(file_id, &name);
    }
}

/// Snapshot of the index at a point in time.
pub struct IndexSnapshot {
    pub file_table: FileTable,
    pub string_arena: StringArena,
    pub trigram_index: TrigramIndex,
}

impl IndexSnapshot {
    /// Save the snapshot to disk.
    pub fn save(&self, path: &Path) -> Result<()> {
        let data = bincode::serialize(&(&self.file_table, &self.string_arena, &self.trigram_index))
            .map_err(|e| vicaya_core::Error::Serialization(e.to_string()))?;

        std::fs::write(path, data)?;
        info!("Index snapshot saved to {}", path.display());
        Ok(())
    }

    /// Load a snapshot from disk.
    pub fn load(path: &Path) -> Result<Self> {
        let data = std::fs::read(path)?;
        let (file_table, string_arena, trigram_index) = bincode::deserialize(&data)
            .map_err(|e| vicaya_core::Error::Serialization(e.to_string()))?;

        info!("Index snapshot loaded from {}", path.display());
        Ok(Self {
            file_table,
            string_arena,
            trigram_index,
        })
    }
}
