//! vicaya-daemon: Background service for vicaya.

use std::path::PathBuf;
use tracing::info;
use vicaya_core::{Config, Result};
use vicaya_scanner::{IndexSnapshot, Scanner};

fn main() -> Result<()> {
    vicaya_core::logging::init();

    info!("vicaya daemon starting...");

    // Load or create default config
    let config = load_config()?;
    config.ensure_index_dir()?;

    // Check if index exists, otherwise build it
    let index_file = config.index_path.join("index.bin");
    let snapshot = if index_file.exists() {
        info!("Loading existing index...");
        IndexSnapshot::load(&index_file)?
    } else {
        info!("Building new index...");
        let scanner = Scanner::new(config.clone());
        let snapshot = scanner.scan()?;
        snapshot.save(&index_file)?;
        snapshot
    };

    info!("Index ready: {} files indexed", snapshot.file_table.len());

    // TODO: Start watcher
    // TODO: Start IPC server

    info!("vicaya daemon running. Press Ctrl+C to stop.");

    // Keep the daemon running
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

fn load_config() -> Result<Config> {
    let config_path = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()))
        .join("Library")
        .join("Application Support")
        .join("vicaya")
        .join("config.toml");

    if config_path.exists() {
        Config::load(&config_path)
    } else {
        let config = Config::default();
        std::fs::create_dir_all(config_path.parent().unwrap())?;
        config.save(&config_path)?;
        Ok(config)
    }
}
