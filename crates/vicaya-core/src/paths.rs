//! Common filesystem paths used by vicaya.

use std::path::PathBuf;

/// Base directory for vicaya state (config, socket, pid, etc).
///
/// Defaults to `~/Library/Application Support/vicaya` on macOS, but can be
/// overridden via `VICAYA_DIR` for testing or multi-instance setups.
pub fn vicaya_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("VICAYA_DIR") {
        return PathBuf::from(dir);
    }

    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("vicaya")
}

/// Path to the vicaya configuration file.
pub fn config_path() -> PathBuf {
    vicaya_dir().join("config.toml")
}

/// Path to the daemon PID file.
pub fn pid_file_path() -> PathBuf {
    vicaya_dir().join("daemon.pid")
}

/// Path to the daemon IPC socket.
pub fn socket_path() -> PathBuf {
    vicaya_dir().join("daemon.sock")
}

#[doc(hidden)]
pub fn test_env_lock() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}
