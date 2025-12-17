//! Configuration management for vicaya.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Main configuration structure for vicaya.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Paths to index (roots).
    pub index_roots: Vec<PathBuf>,

    /// Paths to exclude from indexing.
    pub exclusions: Vec<String>,

    /// Path to store the index data.
    pub index_path: PathBuf,

    /// Maximum memory usage in MB.
    pub max_memory_mb: usize,

    /// Performance settings.
    pub performance: PerformanceConfig,
}

/// Performance-related configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    /// Number of parallel scanner threads.
    pub scanner_threads: usize,

    /// Reconciliation hour (0-23).
    pub reconcile_hour: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            index_roots: vec![PathBuf::from(
                std::env::var("HOME").unwrap_or_else(|_| "/".to_string()),
            )],
            exclusions: vec![
                "/System".to_string(),
                "/Library".to_string(),
                "/.git".to_string(),
                "/node_modules".to_string(),
                "/target".to_string(),
            ],
            index_path: Self::default_index_path(),
            max_memory_mb: 512,
            performance: PerformanceConfig {
                scanner_threads: num_cpus::get(),
                reconcile_hour: 3,
            },
        }
    }
}

impl Config {
    /// Load configuration from a TOML file.
    pub fn load(path: &std::path::Path) -> crate::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let mut config: Self =
            toml::from_str(&content).map_err(|e| crate::Error::Config(e.to_string()))?;

        // Expand tilde (~) and environment variables in paths using shellexpand
        config.expand_paths();

        Ok(config)
    }

    /// Expand tilde (~) and environment variables in all path fields.
    fn expand_paths(&mut self) {
        // Expand in index_roots
        self.index_roots = self
            .index_roots
            .iter()
            .map(|p| Self::expand_path(p.as_ref()))
            .collect();

        // Expand in index_path
        self.index_path = Self::expand_path(&self.index_path);
    }

    /// Expand tilde and environment variables in a single path.
    fn expand_path(path: &Path) -> PathBuf {
        let path_str = path.to_string_lossy();

        // Use shellexpand to handle ~, ~user, and $VAR expansion
        match shellexpand::full(&path_str) {
            Ok(expanded) => PathBuf::from(expanded.as_ref()),
            Err(_) => path.to_path_buf(), // Fallback to original path on error
        }
    }

    /// Save configuration to a TOML file.
    pub fn save(&self, path: &std::path::Path) -> crate::Result<()> {
        let content =
            toml::to_string_pretty(self).map_err(|e| crate::Error::Config(e.to_string()))?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Get the default index path.
    fn default_index_path() -> PathBuf {
        crate::paths::vicaya_dir().join("index")
    }

    /// Ensure the index directory exists.
    pub fn ensure_index_dir(&self) -> crate::Result<()> {
        std::fs::create_dir_all(&self.index_path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_expand_path_with_tilde() {
        let home = env::var("HOME").unwrap();
        let path = Path::new("~/test/path");
        let expanded = Config::expand_path(path);

        assert_eq!(expanded, PathBuf::from(format!("{home}/test/path")));
    }

    #[test]
    fn test_expand_path_with_env_var() {
        env::set_var("TEST_VAR", "/test/location");
        let path = Path::new("$TEST_VAR/subdir");
        let expanded = Config::expand_path(path);

        assert_eq!(expanded, PathBuf::from("/test/location/subdir"));
        env::remove_var("TEST_VAR");
    }

    #[test]
    fn test_expand_path_absolute() {
        let path = Path::new("/absolute/path");
        let expanded = Config::expand_path(path);

        assert_eq!(expanded, PathBuf::from("/absolute/path"));
    }

    #[test]
    fn test_expand_path_relative() {
        let path = Path::new("relative/path");
        let expanded = Config::expand_path(path);

        assert_eq!(expanded, PathBuf::from("relative/path"));
    }

    #[test]
    fn test_expand_paths_in_config() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let home = env::var("HOME").unwrap();

        let config_content = r#"
index_roots = ["~/Documents", "$HOME/Projects"]
exclusions = [".git", "node_modules"]
index_path = "~/Library/Application Support/vicaya"
max_memory_mb = 512

[performance]
scanner_threads = 4
reconcile_hour = 3
"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(config_content.as_bytes()).unwrap();
        temp_file.flush().unwrap();

        let config = Config::load(temp_file.path()).unwrap();

        // Verify tilde expansion in index_roots
        assert_eq!(
            config.index_roots[0],
            PathBuf::from(format!("{home}/Documents"))
        );
        assert_eq!(
            config.index_roots[1],
            PathBuf::from(format!("{home}/Projects"))
        );

        // Verify tilde expansion in index_path
        assert_eq!(
            config.index_path,
            PathBuf::from(format!("{home}/Library/Application Support/vicaya"))
        );
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();

        assert!(!config.index_roots.is_empty());
        assert!(!config.exclusions.is_empty());
        assert_eq!(config.max_memory_mb, 512);
        assert_eq!(config.performance.reconcile_hour, 3);
    }

    #[test]
    fn test_config_save_and_load() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let config_path = dir.path().join("test_config.toml");

        let config = Config {
            index_roots: vec![PathBuf::from("/test/root")],
            exclusions: vec![".git".to_string(), "target".to_string()],
            index_path: PathBuf::from("/test/index"),
            max_memory_mb: 256,
            performance: PerformanceConfig {
                scanner_threads: 8,
                reconcile_hour: 2,
            },
        };

        // Save
        config.save(&config_path).unwrap();

        // Load
        let loaded_config = Config::load(&config_path).unwrap();

        assert_eq!(loaded_config.index_roots, config.index_roots);
        assert_eq!(loaded_config.exclusions, config.exclusions);
        assert_eq!(loaded_config.index_path, config.index_path);
        assert_eq!(loaded_config.max_memory_mb, config.max_memory_mb);
        assert_eq!(
            loaded_config.performance.scanner_threads,
            config.performance.scanner_threads
        );
        assert_eq!(
            loaded_config.performance.reconcile_hour,
            config.performance.reconcile_hour
        );
    }
}
