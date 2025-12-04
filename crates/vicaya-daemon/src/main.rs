//! vicaya-daemon: Background service for vicaya.

mod ipc_server;

use std::path::PathBuf;
use tracing::info;
use vicaya_core::{Config, Result};
use vicaya_scanner::{IndexSnapshot, Scanner};

use crate::ipc_server::IpcServer;

fn main() -> Result<()> {
    vicaya_core::logging::init();

    if std::env::args().any(|arg| arg == "--version" || arg == "-V") {
        println!(
            "{}",
            vicaya_core::build_info::version_string("vicaya-daemon")
        );
        return Ok(());
    }

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

    // Start IPC server
    let socket_path = vicaya_core::ipc::socket_path();
    let server = IpcServer::new(&socket_path, snapshot)?;

    info!("vicaya daemon running. Press Ctrl+C to stop.");

    // Run the IPC server
    server.run()?;

    Ok(())
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
