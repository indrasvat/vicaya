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
            .filter(|e| e.file_type().is_file() || e.file_type().is_dir())
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
        vicaya_core::filter::should_index_path(path, &self.config.exclusions)
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
        if name.is_empty() {
            return;
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use vicaya_core::Config;

    fn make_scanner(exclusions: Vec<String>) -> Scanner {
        let config = Config {
            exclusions,
            ..Default::default()
        };
        Scanner::new(config)
    }

    #[test]
    fn test_should_index_substring_not_excluded() {
        // REGRESSION TEST: "bin" should NOT match "robinsharma" (substring)
        let scanner = make_scanner(vec!["bin".to_string()]);

        assert!(scanner.should_index(Path::new("/Users/robinsharma/Documents/file.txt")));
        assert!(scanner.should_index(Path::new("/home/robin/test.txt")));
        assert!(scanner.should_index(Path::new("/combined/path/file.txt")));
    }

    #[test]
    fn test_should_index_exact_component_excluded() {
        // "bin" SHOULD match an exact component named "bin"
        let scanner = make_scanner(vec!["bin".to_string()]);

        assert!(!scanner.should_index(Path::new("/usr/bin/ls")));
        assert!(!scanner.should_index(Path::new("/home/user/bin/script.sh")));
        assert!(!scanner.should_index(Path::new("/bin/bash")));
    }

    #[test]
    fn test_should_index_hidden_files() {
        let scanner = make_scanner(vec![".git".to_string(), ".DS_Store".to_string()]);

        // Should exclude exact matches
        assert!(!scanner.should_index(Path::new("/home/user/project/.git/config")));
        assert!(!scanner.should_index(Path::new("/Users/test/.DS_Store")));

        // Should NOT exclude when it's just a substring
        assert!(scanner.should_index(Path::new("/home/user/.github/workflows/ci.yml")));
        assert!(scanner.should_index(Path::new("/Users/test/my.DS_Store.bak")));
    }

    #[test]
    fn test_should_index_glob_extension_patterns() {
        let scanner = make_scanner(vec!["*.pyc".to_string(), "*.log".to_string()]);

        // Should exclude files with matching extensions
        assert!(!scanner.should_index(Path::new("/home/user/script.pyc")));
        assert!(!scanner.should_index(Path::new("/var/log/app.log")));
        assert!(!scanner.should_index(Path::new("/path/to/file.pyc")));

        // Should NOT exclude files with different extensions
        assert!(scanner.should_index(Path::new("/home/user/script.py")));
        assert!(scanner.should_index(Path::new("/home/user/mylog.txt")));
        assert!(scanner.should_index(Path::new("/path/to/file.py")));
    }

    #[test]
    fn test_should_index_glob_prefix_patterns() {
        let scanner = make_scanner(vec!["._*".to_string()]);

        // Should exclude files starting with "._"
        assert!(!scanner.should_index(Path::new("/Users/test/._secret")));
        assert!(!scanner.should_index(Path::new("/path/._metadata")));

        // Should NOT exclude files that just contain "._" elsewhere
        assert!(scanner.should_index(Path::new("/Users/test/my._file")));
        assert!(scanner.should_index(Path::new("/Users/test/normal_file")));
    }

    #[test]
    fn test_should_index_nested_exclusions() {
        let scanner = make_scanner(vec!["node_modules".to_string()]);

        // Should exclude anything inside node_modules
        assert!(!scanner.should_index(Path::new(
            "/home/user/project/node_modules/package/index.js"
        )));
        assert!(!scanner.should_index(Path::new("/project/node_modules/deep/nested/file.txt")));

        // Should NOT exclude paths that just contain the substring
        assert!(scanner.should_index(Path::new("/home/user/my_node_modules_backup/file.txt")));
    }

    #[test]
    fn test_should_index_multiple_exclusions() {
        let scanner = make_scanner(vec![
            ".git".to_string(),
            "target".to_string(),
            "*.tmp".to_string(),
        ]);

        // Test each exclusion
        assert!(!scanner.should_index(Path::new("/project/.git/HEAD")));
        assert!(!scanner.should_index(Path::new("/rust/project/target/debug/app")));
        assert!(!scanner.should_index(Path::new("/temp/file.tmp")));

        // Should index everything else
        assert!(scanner.should_index(Path::new("/project/src/main.rs")));
        assert!(scanner.should_index(Path::new("/home/user/document.txt")));
    }

    #[test]
    fn test_should_index_case_sensitive() {
        let scanner = make_scanner(vec!["Build".to_string()]);

        // Should exclude exact case match
        assert!(!scanner.should_index(Path::new("/project/Build/output")));

        // Should NOT exclude different case
        assert!(scanner.should_index(Path::new("/project/build/output")));
        assert!(scanner.should_index(Path::new("/project/BUILD/output")));
    }

    #[test]
    fn test_should_index_common_directories() {
        let scanner = make_scanner(vec![
            ".cache".to_string(),
            ".venv".to_string(),
            "__pycache__".to_string(),
        ]);

        // Should exclude these common directories
        assert!(!scanner.should_index(Path::new("/home/user/.cache/pip/file")));
        assert!(!scanner.should_index(Path::new("/project/.venv/lib/python")));
        assert!(!scanner.should_index(Path::new("/project/__pycache__/module.pyc")));

        // Should NOT exclude similar names
        assert!(scanner.should_index(Path::new("/home/user/my_cache/file")));
        assert!(scanner.should_index(Path::new("/project/venv/lib/python")));
        assert!(scanner.should_index(Path::new("/project/pycache/file")));
    }

    #[test]
    fn test_should_index_edge_cases() {
        let scanner = make_scanner(vec!["*".to_string()]); // Invalid but shouldn't crash

        // Should handle gracefully (doesn't match prefix* or *.ext pattern)
        assert!(scanner.should_index(Path::new("/any/path/file.txt")));
    }

    #[test]
    fn test_should_index_empty_exclusions() {
        let scanner = make_scanner(vec![]);

        // Should index everything when no exclusions
        assert!(scanner.should_index(Path::new("/any/path")));
        assert!(scanner.should_index(Path::new("/.git/config")));
        assert!(scanner.should_index(Path::new("/file.pyc")));
    }

    #[test]
    fn test_should_index_root_components() {
        let scanner = make_scanner(vec!["Users".to_string()]);

        // Should exclude if "Users" is a component
        assert!(!scanner.should_index(Path::new("/Users/test/file.txt")));

        // But root "/" itself should be indexable
        let scanner = make_scanner(vec!["/".to_string()]);
        assert!(scanner.should_index(Path::new("/home/user/file.txt")));
    }
}
