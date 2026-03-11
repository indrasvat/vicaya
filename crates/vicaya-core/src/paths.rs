//! Common filesystem paths used by vicaya.

use std::path::{Path, PathBuf};

use crate::{Error, Result};

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

/// Expand `~` and environment variables in a user-supplied path.
pub fn expand_user_path(path: &Path) -> PathBuf {
    let path_str = path.to_string_lossy();
    match shellexpand::full(&path_str) {
        Ok(expanded) => PathBuf::from(expanded.as_ref()),
        Err(_) => path.to_path_buf(),
    }
}

/// Resolve a user-supplied directory path to an absolute normalized directory.
pub fn resolve_scope_dir(path: &Path) -> Result<PathBuf> {
    let expanded = expand_user_path(path);
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()?.join(expanded)
    };

    let normalized = normalize_absolute_path(&absolute);
    let metadata = std::fs::metadata(&normalized).map_err(|err| {
        Error::Other(format!(
            "Failed to resolve scope directory '{}': {}",
            path.display(),
            err
        ))
    })?;

    if !metadata.is_dir() {
        return Err(Error::Other(format!(
            "Scope path '{}' is not a directory",
            path.display()
        )));
    }

    Ok(normalized)
}

fn normalize_absolute_path(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::RootDir | Component::Prefix(_) | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

#[doc(hidden)]
pub fn test_env_lock() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_user_path_preserves_relative_paths() {
        assert_eq!(expand_user_path(Path::new("./foo")), PathBuf::from("./foo"));
    }

    #[test]
    fn expand_user_path_expands_home_prefix() {
        let _lock = test_env_lock();
        let home = std::env::var("HOME").expect("HOME should be set");
        assert_eq!(
            expand_user_path(Path::new("~/Documents")),
            PathBuf::from(home).join("Documents")
        );
    }

    #[test]
    fn resolve_scope_dir_canonicalizes_relative_directories() {
        let dir = tempfile::tempdir().unwrap();
        let old_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let expected_root = std::env::current_dir().unwrap();

        std::fs::create_dir_all("alpha/beta").unwrap();
        let resolved = resolve_scope_dir(Path::new("./alpha/../alpha/beta")).unwrap();

        std::env::set_current_dir(old_cwd).unwrap();

        assert_eq!(resolved, expected_root.join("alpha/beta"));
    }

    #[test]
    fn resolve_scope_dir_rejects_files() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note.txt");
        std::fs::write(&file, "").unwrap();

        let err = resolve_scope_dir(&file).unwrap_err();
        assert!(
            err.to_string().contains("not a directory"),
            "unexpected error: {err}"
        );
    }
}
