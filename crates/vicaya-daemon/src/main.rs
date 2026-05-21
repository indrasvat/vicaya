//! vicaya-daemon: Background service for vicaya.

mod ipc_server;

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};
use tracing::{info, warn};
use vicaya_core::{Config, Result};
use vicaya_scanner::{IndexSnapshot, Scanner};
use vicaya_watcher::{FileWatcher, IndexUpdate};

use crate::ipc_server::{
    prepare_index_update, DaemonState, IpcServer, PreparedIndexUpdate, SharedState,
};

const WATCHER_APPLY_CHUNK_SIZE: usize = 256;

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
    let had_index = index_file.exists();
    let snapshot = if had_index {
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

    // Fresh scans are authoritative. Existing snapshots become live immediately;
    // startup reconcile catches downtime changes and truncates any stale journal
    // after the IPC socket is ready.
    if !had_index {
        clear_stale_journal(&journal_file)?;
    }

    let shutdown = Arc::new(AtomicBool::new(false));
    let journal_lock = Arc::new(Mutex::new(()));
    let rebuild_lock = Arc::new(Mutex::new(()));

    // Start IPC server first to ensure single-instance semantics.
    let socket_path = vicaya_core::ipc::socket_path();
    let server = IpcServer::new(
        &socket_path,
        Arc::clone(&state),
        Arc::clone(&shutdown),
        Arc::clone(&journal_lock),
        Arc::clone(&rebuild_lock),
    )?;

    // Record PID once we're successfully bound.
    vicaya_core::daemon::write_pid(std::process::id() as i32)?;

    // Start watcher thread
    let watcher_thread = start_watcher_thread(
        config.clone(),
        Arc::clone(&state),
        Arc::clone(&shutdown),
        Arc::clone(&journal_lock),
    )?;

    // Start reconciliation thread to catch up on missed updates during downtime.
    let reconcile_thread = start_reconcile_thread(
        config.clone(),
        Arc::clone(&state),
        Arc::clone(&shutdown),
        Arc::clone(&journal_lock),
        Arc::clone(&rebuild_lock),
        had_index,
    )?;

    info!("vicaya daemon running. Press Ctrl+C to stop.");

    // Run the IPC server (blocks until shutdown)
    let server_result = server.run();

    shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Err(e) = watcher_thread.join() {
        warn!("Watcher thread did not shut down cleanly: {:?}", e);
    }
    if let Err(e) = reconcile_thread.join() {
        warn!("Reconcile thread did not shut down cleanly: {:?}", e);
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

fn clear_stale_journal(journal_file: &Path) -> Result<()> {
    if !journal_file.exists() {
        return Ok(());
    }

    let file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(journal_file)?;
    drop(file);

    info!(
        "Discarded stale journal history after fresh index build: {}",
        journal_file.display()
    );
    Ok(())
}

fn start_watcher_thread(
    config: Config,
    state: SharedState,
    shutdown: Arc<AtomicBool>,
    journal_lock: Arc<Mutex<()>>,
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

            {
                let _guard = journal_lock.lock().unwrap();
                if let Err(e) = append_journal(&journal_file, &updates) {
                    warn!("Failed to append journal: {}", e);
                }
            }

            apply_watcher_updates(&state, updates);
        }

        info!("Watcher thread exiting");
    });

    Ok(handle)
}

fn apply_watcher_updates(state: &SharedState, updates: Vec<IndexUpdate>) {
    let config = { state.read().unwrap().config.clone() };
    let updates = prepare_watcher_updates(&config, updates);
    apply_watcher_updates_chunked(state, updates, WATCHER_APPLY_CHUNK_SIZE, |_| {
        std::thread::yield_now();
    });
}

fn prepare_watcher_updates(config: &Config, updates: Vec<IndexUpdate>) -> Vec<PreparedIndexUpdate> {
    updates
        .into_iter()
        .map(|update| prepare_index_update(config, update))
        .collect()
}

fn apply_watcher_updates_chunked<F>(
    state: &SharedState,
    updates: Vec<PreparedIndexUpdate>,
    chunk_size: usize,
    mut after_chunk: F,
) where
    F: FnMut(usize),
{
    let chunk_size = chunk_size.max(1);
    let chunk_count = updates.len().div_ceil(chunk_size);

    for (idx, chunk) in updates.chunks(chunk_size).enumerate() {
        {
            let mut state = state.write().unwrap();
            for update in chunk {
                state.apply_prepared_update(update.clone());
            }
        }

        if idx + 1 < chunk_count {
            after_chunk(idx + 1);
        }
    }
}

fn start_reconcile_thread(
    config: Config,
    state: SharedState,
    shutdown: Arc<AtomicBool>,
    journal_lock: Arc<Mutex<()>>,
    rebuild_lock: Arc<Mutex<()>>,
    had_index: bool,
) -> Result<std::thread::JoinHandle<()>> {
    let handle = std::thread::spawn(move || {
        if had_index && !shutdown.load(std::sync::atomic::Ordering::Relaxed) {
            // Initial reconcile: discover pre-existing files that won't emit watcher events.
            if let Err(e) =
                crate::ipc_server::full_rebuild_from_disk(&state, &journal_lock, &rebuild_lock)
            {
                warn!("Initial reconcile failed: {}", e);
                let mut state = state.write().unwrap();
                state.reconciling = false;
            }
        }

        // Scheduled daily reconciliation for resilience against missed watcher events.
        loop {
            if shutdown.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }

            let sleep_for = next_reconcile_sleep(config.performance.reconcile_hour);
            let mut slept = std::time::Duration::from_secs(0);
            while slept < sleep_for {
                if shutdown.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                let step = std::cmp::min(std::time::Duration::from_millis(250), sleep_for - slept);
                std::thread::sleep(step);
                slept += step;
            }

            if shutdown.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }

            if let Err(e) =
                crate::ipc_server::full_rebuild_from_disk(&state, &journal_lock, &rebuild_lock)
            {
                warn!("Scheduled reconcile failed: {}", e);
                let mut state = state.write().unwrap();
                state.reconciling = false;
            }
        }
    });

    Ok(handle)
}

fn next_reconcile_sleep(reconcile_hour: u8) -> std::time::Duration {
    use chrono::{Local, TimeZone};

    let now = Local::now();
    let today = now.date_naive();
    let hour = reconcile_hour as u32;

    let naive_today = today
        .and_hms_opt(hour, 0, 0)
        .unwrap_or_else(|| today.and_hms_opt(3, 0, 0).unwrap());

    let mut target = Local
        .from_local_datetime(&naive_today)
        .earliest()
        .unwrap_or(now);

    if target <= now {
        let tomorrow = today + chrono::Duration::days(1);
        let naive_tomorrow = tomorrow
            .and_hms_opt(hour, 0, 0)
            .unwrap_or_else(|| tomorrow.and_hms_opt(3, 0, 0).unwrap());
        target = Local
            .from_local_datetime(&naive_tomorrow)
            .earliest()
            .unwrap_or(now + chrono::Duration::hours(24));
    }

    let delta = target - now;
    delta.to_std().unwrap_or(std::time::Duration::from_secs(0))
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use vicaya_core::config::PerformanceConfig;

    fn test_config(root: &Path, vicaya_dir: &Path) -> Config {
        Config {
            index_roots: vec![root.to_path_buf()],
            exclusions: vec![],
            respect_ignore_files: true,
            index_path: vicaya_dir.join("index"),
            max_memory_mb: 128,
            performance: PerformanceConfig {
                scanner_threads: 2,
                reconcile_hour: 3,
            },
        }
    }

    fn build_state(root: &Path, vicaya_dir: &Path) -> SharedState {
        let config = test_config(root, vicaya_dir);
        std::fs::create_dir_all(&config.index_path).unwrap();
        let snapshot = Scanner::new(config.clone()).scan().unwrap();
        Arc::new(RwLock::new(DaemonState::new(
            config,
            vicaya_dir.join("index.bin"),
            vicaya_dir.join("journal.log"),
            snapshot,
        )))
    }

    fn state_contains_path(state: &DaemonState, path: &Path) -> bool {
        let needle = path.to_string_lossy();
        state.snapshot.file_table.iter().any(|(_, meta)| {
            if meta.path_len == 0 {
                return false;
            }

            state
                .snapshot
                .string_arena
                .get(meta.path_offset, meta.path_len)
                .is_some_and(|indexed| indexed == needle)
        })
    }

    fn public_indexed_count(state: &DaemonState) -> usize {
        state.path_to_id.len()
            + state
                .path_hash_collisions
                .values()
                .map(|ids| ids.len())
                .sum::<usize>()
    }

    #[test]
    fn watcher_updates_release_state_lock_between_chunks() {
        let vicaya_dir = tempdir().unwrap();
        let root = tempdir().unwrap();
        let state = build_state(root.path(), vicaya_dir.path());

        let first = root.path().join("one.txt");
        let second = root.path().join("two.txt");
        std::fs::write(&first, "one").unwrap();
        std::fs::write(&second, "two").unwrap();

        let updates = prepare_watcher_updates(
            &state.read().unwrap().config,
            vec![
                IndexUpdate::Create {
                    path: first.to_string_lossy().to_string(),
                },
                IndexUpdate::Create {
                    path: second.to_string_lossy().to_string(),
                },
            ],
        );
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        let worker_state = Arc::clone(&state);

        let worker = std::thread::spawn(move || {
            apply_watcher_updates_chunked(&worker_state, updates, 1, |chunk| {
                if chunk == 1 {
                    ready_tx.send(()).unwrap();
                    resume_rx.recv().unwrap();
                }
            });
        });

        ready_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        {
            let state = state.read().unwrap();
            assert!(public_indexed_count(&state) >= 1);
        }
        resume_tx.send(()).unwrap();
        worker.join().unwrap();

        let state = state.read().unwrap();
        assert!(state_contains_path(&state, &first));
        assert!(state_contains_path(&state, &second));
    }

    #[test]
    fn internal_update_filter_rejects_vicaya_state_paths() {
        let internal_dir = Path::new("/tmp/vicaya");
        let index_dir = Path::new("/tmp/vicaya/index");
        let indexed_path = "/tmp/repo/src/main.rs".to_string();
        let internal_path = "/tmp/vicaya/daemon.sock".to_string();
        let index_path = "/tmp/vicaya/index/index.journal".to_string();

        assert!(!is_internal_update(
            &IndexUpdate::Modify { path: indexed_path },
            internal_dir,
            index_dir
        ));
        assert!(is_internal_update(
            &IndexUpdate::Modify {
                path: internal_path
            },
            internal_dir,
            index_dir
        ));
        assert!(is_internal_update(
            &IndexUpdate::Create { path: index_path },
            internal_dir,
            index_dir
        ));
        assert!(is_internal_update(
            &IndexUpdate::Move {
                from: "/tmp/repo/a".to_string(),
                to: "/tmp/vicaya/index/index.bin".to_string(),
            },
            internal_dir,
            index_dir
        ));
    }
}
