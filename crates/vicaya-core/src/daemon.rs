//! Daemon lifecycle management utilities.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Get the PID file path for the daemon.
pub fn pid_file_path() -> PathBuf {
    crate::paths::pid_file_path()
}

/// Check if the daemon is currently running.
pub fn is_running() -> bool {
    let pid_file = pid_file_path();

    if !pid_file.exists() {
        return is_socket_connectable();
    }

    // Read PID from file
    let pid = match fs::read_to_string(&pid_file) {
        Ok(content) => match content.trim().parse::<i32>() {
            Ok(pid) => pid,
            Err(_) => return false,
        },
        Err(_) => return false,
    };

    // Check if process exists
    #[cfg(unix)]
    {
        // Send signal 0 to check if process exists without killing it
        let running = unsafe { libc::kill(pid, 0) == 0 };
        if running {
            return true;
        }
    }

    // If PID file exists but process does not, consider it stale and fall back to socket check.
    // If the socket isn't connectable either, remove the stale PID file.
    let socket_ok = is_socket_connectable();
    if !socket_ok {
        let _ = remove_pid_file();
    }
    socket_ok
}

/// Get the PID of the running daemon, if any.
pub fn get_pid() -> Option<i32> {
    let pid_file = pid_file_path();

    if !pid_file.exists() {
        return None;
    }

    fs::read_to_string(&pid_file)
        .ok()?
        .trim()
        .parse::<i32>()
        .ok()
}

/// Write the daemon PID to the PID file.
pub fn write_pid(pid: i32) -> std::io::Result<()> {
    let pid_file = pid_file_path();

    // Create parent directory if it doesn't exist
    if let Some(parent) = pid_file.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(&pid_file, pid.to_string())?;
    Ok(())
}

/// Remove the PID file.
pub fn remove_pid_file() -> std::io::Result<()> {
    let pid_file = pid_file_path();

    if pid_file.exists() {
        fs::remove_file(&pid_file)?;
    }

    Ok(())
}

/// Start the daemon in the background.
pub fn start_daemon() -> crate::Result<i32> {
    if is_running() {
        return Err(crate::Error::Config(
            "Daemon is already running".to_string(),
        ));
    }

    // Find the vicaya-daemon binary
    let daemon_path = find_daemon_binary()?;

    // Start daemon as a background process
    #[cfg(unix)]
    {
        #[allow(unused_imports)]
        use std::os::unix::process::CommandExt;

        let child = Command::new(&daemon_path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            // Daemonize: set process group
            .process_group(0)
            .spawn()
            .map_err(crate::Error::Io)?;

        let pid = child.id() as i32;

        wait_for_daemon_ready(pid)?;

        // Ensure PID file exists for consumers that rely on it.
        if !pid_file_path().exists() {
            write_pid(pid).map_err(crate::Error::Io)?;
        }

        Ok(pid)
    }

    #[cfg(not(unix))]
    {
        Err(crate::Error::Config(
            "Daemon start not supported on this platform".to_string(),
        ))
    }
}

/// Stop the daemon gracefully.
pub fn stop_daemon() -> crate::Result<()> {
    if !is_running() {
        return Err(crate::Error::Config("Daemon is not running".to_string()));
    }

    // Try a graceful shutdown via IPC first.
    let _ = request_shutdown_via_ipc();

    let pid = get_pid();

    #[cfg(unix)]
    {
        // Wait for daemon to shut down (with timeout).
        // Prefer PID-based checks if we have one; otherwise fall back to socket connectivity.
        for _ in 0..50 {
            if let Some(pid) = pid {
                if unsafe { libc::kill(pid, 0) != 0 } {
                    let _ = remove_pid_file();
                    return Ok(());
                }
            } else if !is_socket_connectable() {
                let _ = remove_pid_file();
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        // Fallback: force terminate if still running and we have a PID.
        if let Some(pid) = pid {
            unsafe {
                if libc::kill(pid, libc::SIGTERM) != 0 {
                    return Err(crate::Error::Config(
                        "Failed to send termination signal to daemon".to_string(),
                    ));
                }
            }
        }

        for _ in 0..50 {
            if !is_running() {
                let _ = remove_pid_file();
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        Err(crate::Error::Config("Daemon did not shut down".to_string()))
    }

    #[cfg(not(unix))]
    {
        Err(crate::Error::Config(
            "Daemon stop not supported on this platform".to_string(),
        ))
    }
}

fn is_socket_connectable() -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::net::UnixStream;
        UnixStream::connect(crate::ipc::socket_path()).is_ok()
    }

    #[cfg(not(unix))]
    {
        false
    }
}

fn wait_for_daemon_ready(pid: i32) -> crate::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::net::UnixStream;
        use std::time::{Duration, Instant};

        // Startup can be dominated by loading a large on-disk index snapshot, so
        // allow a generous timeout before declaring failure.
        //
        // Note: readiness is defined as "the IPC socket is connectable".
        let deadline = Instant::now() + Duration::from_secs(30);

        while Instant::now() < deadline {
            // If the process died, bail early.
            if unsafe { libc::kill(pid, 0) != 0 } {
                return Err(crate::Error::Config(
                    "Daemon exited during startup".to_string(),
                ));
            }

            if UnixStream::connect(crate::ipc::socket_path()).is_ok() {
                return Ok(());
            }

            std::thread::sleep(Duration::from_millis(50));
        }

        Err(crate::Error::Config(
            "Timed out waiting for daemon to become ready".to_string(),
        ))
    }

    #[cfg(not(unix))]
    {
        let _ = pid;
        Err(crate::Error::Config(
            "Daemon start not supported on this platform".to_string(),
        ))
    }
}

fn request_shutdown_via_ipc() -> crate::Result<()> {
    #[cfg(unix)]
    {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;

        let mut stream = UnixStream::connect(crate::ipc::socket_path())
            .map_err(|e| crate::Error::Ipc(format!("Failed to connect to daemon: {}", e)))?;

        let mut request_json = crate::ipc::Request::Shutdown
            .to_json()
            .map_err(|e| crate::Error::Ipc(format!("Failed to serialize request: {}", e)))?;
        request_json.push('\n');

        stream
            .write_all(request_json.as_bytes())
            .map_err(|e| crate::Error::Ipc(format!("Failed to send request: {}", e)))?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        let _ = reader.read_line(&mut line);
        Ok(())
    }

    #[cfg(not(unix))]
    {
        Err(crate::Error::Config(
            "Daemon stop not supported on this platform".to_string(),
        ))
    }
}

/// Find the vicaya-daemon binary.
fn find_daemon_binary() -> crate::Result<PathBuf> {
    // Allow tests and advanced users to pin an exact daemon binary.
    if let Ok(path) = std::env::var("VICAYA_DAEMON_BIN") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Ok(path);
        }
    }

    // Prefer a sibling `vicaya-daemon` next to the current executable (common
    // in development builds where both binaries live in `target/*/`).
    if let Ok(current) = std::env::current_exe() {
        if let Some(dir) = current.parent() {
            let candidate = dir.join(format!("vicaya-daemon{}", std::env::consts::EXE_SUFFIX));
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    // Try to find in PATH
    if let Ok(output) = Command::new("which").arg("vicaya-daemon").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(PathBuf::from(path));
            }
        }
    }

    // Try common installation locations
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let cargo_bin = PathBuf::from(&home)
        .join(".cargo")
        .join("bin")
        .join("vicaya-daemon");

    if cargo_bin.exists() {
        return Ok(cargo_bin);
    }

    Err(crate::Error::Config(
        "Could not find vicaya-daemon binary. Please ensure it's installed.".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_test_vicaya_dir<T>(f: impl FnOnce(&std::path::Path) -> T) -> T {
        let _lock = crate::paths::test_env_lock();
        let dir = tempfile::tempdir().expect("Should create temp dir");
        std::env::set_var("VICAYA_DIR", dir.path());

        let result = f(dir.path());

        std::env::remove_var("VICAYA_DIR");
        result
    }

    #[test]
    fn test_pid_file_path() {
        with_test_vicaya_dir(|dir| {
            let path = pid_file_path();
            assert_eq!(path, dir.join("daemon.pid"));
        });
    }

    #[test]
    fn test_write_and_read_pid() {
        with_test_vicaya_dir(|_| {
            let test_pid = 12345;
            write_pid(test_pid).unwrap();

            let read_pid = get_pid();
            assert_eq!(read_pid, Some(test_pid));

            // Cleanup
            remove_pid_file().unwrap();
        });
    }
}
