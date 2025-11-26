//! Configuration management for vicaya.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
            .map(Self::expand_path)
            .collect();

        // Expand in index_path
        self.index_path = Self::expand_path(&self.index_path);
    }

    /// Expand tilde and environment variables in a single path.
    fn expand_path(path: &PathBuf) -> PathBuf {
        let path_str = path.to_string_lossy();

        // Use shellexpand to handle ~, ~user, and $VAR expansion
        match shellexpand::full(&path_str) {
            Ok(expanded) => PathBuf::from(expanded.as_ref()),
            Err(_) => path.clone(), // Fallback to original path on error
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
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("vicaya")
    }

    /// Ensure the index directory exists.
    pub fn ensure_index_dir(&self) -> crate::Result<()> {
        std::fs::create_dir_all(&self.index_path)?;
        Ok(())
    }
}
