//! vicaya-daemon: Background service for vicaya.

mod ipc_server;

use std::io::BufRead;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};
use tracing::{info, warn};
use vicaya_core::{Config, Result};
use vicaya_scanner::{IndexSnapshot, Scanner};
use vicaya_watcher::FileWatcher;

use crate::ipc_server::{DaemonState, IpcServer, SharedState};

fn main() -> Result<()> {
    vicaya_core::logging::init();

    if std::env::args().any(|arg| arg == "--version" || arg == "-V") {
        println!(
            "{}",
            vicaya_core::build_info::BUILD_INFO.version_line("vicaya-daemon")
        );
        return Ok(());
    }

    info!("vicaya daemon starting...");

    // Load or create default config
    let config = load_config()?;
    config.ensure_index_dir()?;

    let index_file = config.index_path.join("index.bin");
    let journal_file = config.index_path.join("index.journal");

    // Check if index exists, otherwise build it
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

    let state: SharedState = Arc::new(RwLock::new(DaemonState::new(
        config.clone(),
        index_file.clone(),
        journal_file.clone(),
        snapshot,
    )));

    // Replay journal (if any) to ensure we don't lose updates across restarts.
    replay_journal(&state, &journal_file)?;

    let shutdown = Arc::new(AtomicBool::new(false));

    // Start IPC server first to ensure single-instance semantics.
    let socket_path = vicaya_core::ipc::socket_path();
    let server = IpcServer::new(&socket_path, Arc::clone(&state), Arc::clone(&shutdown))?;

    // Record PID once we're successfully bound.
    vicaya_core::daemon::write_pid(std::process::id() as i32)?;

    // Start watcher thread
    let watcher_thread =
        start_watcher_thread(config.clone(), Arc::clone(&state), Arc::clone(&shutdown))?;

    info!("vicaya daemon running. Press Ctrl+C to stop.");

    // Run the IPC server (blocks until shutdown)
    let server_result = server.run();

    shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Err(e) = watcher_thread.join() {
        warn!("Watcher thread did not shut down cleanly: {:?}", e);
    }

    // Best-effort cleanup.
    let _ = vicaya_core::daemon::remove_pid_file();
    let _ = std::fs::remove_file(&socket_path);

    server_result
}

fn load_config() -> Result<Config> {
    let config_path = vicaya_core::paths::config_path();

    if config_path.exists() {
        Config::load(&config_path)
    } else {
        let config = Config::default();
        std::fs::create_dir_all(config_path.parent().unwrap())?;
        config.save(&config_path)?;
        Ok(config)
    }
}

fn replay_journal(state: &SharedState, journal_file: &Path) -> Result<()> {
    if !journal_file.exists() {
        return Ok(());
    }

    let file = std::fs::File::open(journal_file)?;
    let reader = std::io::BufReader::new(file);

    let mut updates = Vec::new();
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                warn!("Failed to read journal line: {}", e);
                continue;
            }
        };

        match serde_json::from_str::<vicaya_watcher::IndexUpdate>(&line) {
            Ok(update) => updates.push(update),
            Err(e) => warn!("Skipping invalid journal entry: {}", e),
        }
    }

    if updates.is_empty() {
        return Ok(());
    }

    info!("Replaying {} journal updates...", updates.len());
    let mut state = state.write().unwrap();
    for update in updates {
        state.apply_update(update);
    }
    Ok(())
}

fn start_watcher_thread(
    config: Config,
    state: SharedState,
    shutdown: Arc<AtomicBool>,
) -> Result<std::thread::JoinHandle<()>> {
    let watcher = FileWatcher::new(&config.index_roots)?;
    let internal_dir = vicaya_core::paths::vicaya_dir();
    let index_dir = config.index_path.clone();
    let journal_file = config.index_path.join("index.journal");

    let handle = std::thread::spawn(move || {
        while !shutdown.load(std::sync::atomic::Ordering::Relaxed) {
            let mut updates = watcher.poll_updates();

            // Avoid feedback loops and indexing internal state.
            updates.retain(|u| !is_internal_update(u, &internal_dir, &index_dir));

            if updates.is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(50));
                continue;
            }

            if let Err(e) = append_journal(&journal_file, &updates) {
                warn!("Failed to append journal: {}", e);
            }

            let mut state = state.write().unwrap();
            for update in updates {
                state.apply_update(update);
            }
        }

        info!("Watcher thread exiting");
    });

    Ok(handle)
}

fn append_journal(path: &Path, updates: &[vicaya_watcher::IndexUpdate]) -> std::io::Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    for update in updates {
        let line = serde_json::to_string(update).unwrap_or_default();
        if line.is_empty() {
            continue;
        }
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
    }
    file.flush()?;
    Ok(())
}

fn is_internal_update(
    update: &vicaya_watcher::IndexUpdate,
    internal_dir: &Path,
    index_dir: &Path,
) -> bool {
    fn is_internal_path(path: &str, internal_dir: &Path, index_dir: &Path) -> bool {
        let p = Path::new(path);
        p.starts_with(internal_dir) || p.starts_with(index_dir)
    }

    match update {
        vicaya_watcher::IndexUpdate::Create { path }
        | vicaya_watcher::IndexUpdate::Modify { path }
        | vicaya_watcher::IndexUpdate::Delete { path } => {
            is_internal_path(path, internal_dir, index_dir)
        }
        vicaya_watcher::IndexUpdate::Move { from, to } => {
            is_internal_path(from, internal_dir, index_dir)
                || is_internal_path(to, internal_dir, index_dir)
        }
    }
}
