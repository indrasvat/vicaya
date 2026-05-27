//! vicaya-scanner: Parallel filesystem scanner.

use ignore::gitignore::GitignoreBuilder;
use std::path::Path;
use tracing::{debug, info, warn};
use vicaya_core::{Config, Result};
use vicaya_index::{FileMeta, FileTable, StringArena, TrigramIndex};

/// Scanned file information.
pub struct ScannedFile {
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
        let mut scanned_entries = 0usize;
        let mut entry_errors = 0usize;
        let exclusions = self.config.exclusions.clone();
        let mut walker = ignore::WalkBuilder::new(root);
        walker
            .follow_links(false)
            .hidden(false)
            .ignore(self.config.respect_ignore_files)
            .git_ignore(self.config.respect_ignore_files)
            .git_global(false)
            .git_exclude(self.config.respect_ignore_files)
            .require_git(false)
            .filter_entry(move |entry| {
                vicaya_core::filter::should_index_path(entry.path(), &exclusions)
            });

        for entry in walker.build() {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => {
                    entry_errors += 1;
                    continue;
                }
            };

            let Some(file_type) = entry.file_type() else {
                continue;
            };
            if !(file_type.is_file() || file_type.is_dir()) {
                continue;
            }

            scanned_entries += 1;
            if let Some(scanned) = self.scan_file(entry.path()) {
                self.add_to_index(
                    entry.path(),
                    scanned,
                    file_table,
                    string_arena,
                    trigram_index,
                );
            }
        }

        if entry_errors > 0 {
            warn!(
                "Skipped {} unreadable entries under {} (permissions?)",
                entry_errors,
                root.display()
            );
        }

        debug!("Scanned {} entries in {}", scanned_entries, root.display());

        Ok(())
    }

    /// Check if a path should be indexed.
    #[cfg(test)]
    fn should_index(&self, path: &Path) -> bool {
        should_index_path(&self.config, path, path.is_dir())
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
            size: metadata.len(),
            mtime,
            dev: metadata.dev(),
            ino: metadata.ino(),
        })
    }

    /// Add a scanned file to the index structures.
    fn add_to_index(
        &self,
        path: &Path,
        file: ScannedFile,
        file_table: &mut FileTable,
        string_arena: &mut StringArena,
        trigram_index: &mut TrigramIndex,
    ) {
        let path_str = path.to_string_lossy();
        let name = path
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

/// Check if a path should be indexed under the same high-level rules used by
/// the scanner. This is also used by the daemon for incremental watcher events.
pub fn should_index_path(config: &Config, path: &Path, is_dir: bool) -> bool {
    vicaya_core::filter::should_index_path(path, &config.exclusions)
        && !is_ignored_by_repo_rules(config, path, is_dir)
}

fn is_ignored_by_repo_rules(config: &Config, path: &Path, is_dir: bool) -> bool {
    if !config.respect_ignore_files {
        return false;
    }

    let Some(root) = matching_index_root(config, path) else {
        return false;
    };
    let Some(stop_at) = path.parent() else {
        return false;
    };

    let mut ignored = false;
    let mut dirs = Vec::new();
    let mut current = stop_at;
    loop {
        if current.starts_with(root) {
            dirs.push(current.to_path_buf());
        }
        if current == root {
            break;
        }
        let Some(parent) = current.parent() else {
            break;
        };
        current = parent;
    }
    dirs.reverse();

    for dir in dirs {
        ignored = apply_ignore_file(&dir.join(".gitignore"), &dir, path, is_dir, ignored);
        ignored = apply_ignore_file(&dir.join(".ignore"), &dir, path, is_dir, ignored);
        ignored = apply_ignore_file(&dir.join(".git/info/exclude"), &dir, path, is_dir, ignored);
    }

    ignored
}

fn matching_index_root<'a>(config: &'a Config, path: &Path) -> Option<&'a Path> {
    config
        .index_roots
        .iter()
        .filter(|root| path.starts_with(root))
        .max_by_key(|root| root.components().count())
        .map(|root| root.as_path())
}

fn apply_ignore_file(
    ignore_file: &Path,
    ignore_root: &Path,
    path: &Path,
    is_dir: bool,
    current: bool,
) -> bool {
    if !ignore_file.is_file() {
        return current;
    }

    let mut builder = GitignoreBuilder::new(ignore_root);
    if let Some(err) = builder.add(ignore_file) {
        warn!(
            "Failed to read ignore file {}: {}",
            ignore_file.display(),
            err
        );
        return current;
    }
    let matcher = match builder.build() {
        Ok(matcher) => matcher,
        Err(err) => {
            warn!(
                "Failed to parse ignore file {}: {}",
                ignore_file.display(),
                err
            );
            return current;
        }
    };

    match matcher.matched_path_or_any_parents(path, is_dir) {
        ignore::Match::Ignore(_) => true,
        ignore::Match::Whitelist(_) => false,
        ignore::Match::None => current,
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
        use std::io::{BufWriter, Write};

        let file = std::fs::File::create(path)?;
        let mut writer = BufWriter::new(file);

        bincode::serialize_into(
            &mut writer,
            &(&self.file_table, &self.string_arena, &self.trigram_index),
        )
        .map_err(|e| vicaya_core::Error::Serialization(e.to_string()))?;

        writer.flush()?;
        info!("Index snapshot saved to {}", path.display());
        Ok(())
    }

    /// Load a snapshot from disk.
    pub fn load(path: &Path) -> Result<Self> {
        use std::io::BufReader;

        let file = std::fs::File::open(path)?;
        let reader = BufReader::new(file);

        let (file_table, string_arena, trigram_index) = bincode::deserialize_from(reader)
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

    fn test_config(root: &Path, respect_ignore_files: bool) -> Config {
        Config {
            index_roots: vec![root.to_path_buf()],
            exclusions: Vec::new(),
            respect_ignore_files,
            index_path: root.join(".vicaya-index"),
            max_memory_mb: 128,
            performance: vicaya_core::config::PerformanceConfig {
                scanner_threads: 2,
                reconcile_hour: 3,
            },
            smriti: vicaya_core::config::SmritiConfig::default(),
        }
    }

    fn indexed_names(snapshot: &IndexSnapshot) -> Vec<String> {
        snapshot
            .file_table
            .iter()
            .filter_map(|(_, meta)| {
                snapshot
                    .string_arena
                    .get(meta.name_offset, meta.name_len)
                    .map(str::to_string)
            })
            .collect()
    }

    #[test]
    fn scan_respects_gitignore_files_by_default() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join(".gitignore"),
            "ignored/\n*.log\n!important.log\n",
        )
        .unwrap();
        std::fs::create_dir(root.path().join("ignored")).unwrap();
        std::fs::write(root.path().join("ignored/skip.rs"), "").unwrap();
        std::fs::write(root.path().join("app.log"), "").unwrap();
        std::fs::write(root.path().join("important.log"), "").unwrap();
        std::fs::write(root.path().join("keep.rs"), "").unwrap();

        let snapshot = Scanner::new(test_config(root.path(), true)).scan().unwrap();
        let names = indexed_names(&snapshot);

        assert!(names.contains(&"keep.rs".to_string()));
        assert!(names.contains(&"important.log".to_string()));
        assert!(!names.contains(&"skip.rs".to_string()));
        assert!(!names.contains(&"app.log".to_string()));
    }

    #[test]
    fn gitignore_support_can_be_disabled() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(".gitignore"), "ignored/\n*.log\n").unwrap();
        std::fs::create_dir(root.path().join("ignored")).unwrap();
        std::fs::write(root.path().join("ignored/skip.rs"), "").unwrap();
        std::fs::write(root.path().join("app.log"), "").unwrap();

        let snapshot = Scanner::new(test_config(root.path(), false))
            .scan()
            .unwrap();
        let names = indexed_names(&snapshot);

        assert!(names.contains(&"skip.rs".to_string()));
        assert!(names.contains(&"app.log".to_string()));
    }

    #[test]
    fn watcher_filter_uses_gitignore_rules_for_incremental_paths() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(".gitignore"), "generated/\n").unwrap();
        std::fs::create_dir(root.path().join("generated")).unwrap();
        let ignored = root.path().join("generated/schema.rs");
        std::fs::write(&ignored, "").unwrap();

        let config = test_config(root.path(), true);

        assert!(!should_index_path(&config, &ignored, false));
    }

    #[test]
    fn test_should_index_substring_not_excluded() {
        // REGRESSION TEST: "bin" should NOT match a username containing it as a substring.
        let scanner = make_scanner(vec!["bin".to_string()]);

        assert!(scanner.should_index(Path::new("/Users/examplebinuser/Documents/file.txt")));
        assert!(scanner.should_index(Path::new("/home/examplebinuser/test.txt")));
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
    fn test_should_index_leading_slash_exclusions() {
        let scanner = make_scanner(vec!["/target".to_string(), "/node_modules".to_string()]);

        assert!(!scanner.should_index(Path::new("/rust/project/target/debug/app")));
        assert!(!scanner.should_index(Path::new(
            "/home/user/project/node_modules/package/index.js"
        )));
        assert!(scanner.should_index(Path::new("/home/user/project/src/main.rs")));
    }

    #[test]
    fn test_should_index_leading_slash_globs() {
        let scanner = make_scanner(vec!["/*.log".to_string(), "/._*".to_string()]);

        assert!(!scanner.should_index(Path::new("/var/log/app.log")));
        assert!(!scanner.should_index(Path::new("/Users/test/._secret")));
        assert!(scanner.should_index(Path::new("/Users/test/app.rs")));
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
