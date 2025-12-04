//! Daemon lifecycle management utilities.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Get the PID file path for the daemon.
pub fn pid_file_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("vicaya")
        .join("daemon.pid")
}

/// Check if the daemon is currently running.
pub fn is_running() -> bool {
    let pid_file = pid_file_path();

    if !pid_file.exists() {
        return false;
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
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[cfg(not(unix))]
    {
        // Fallback for non-Unix systems (not fully supported)
        false
    }
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
        write_pid(pid).map_err(crate::Error::Io)?;

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

    let pid =
        get_pid().ok_or_else(|| crate::Error::Config("Could not read daemon PID".to_string()))?;

    #[cfg(unix)]
    {
        // Send SIGTERM to daemon
        unsafe {
            if libc::kill(pid, libc::SIGTERM) != 0 {
                return Err(crate::Error::Config(
                    "Failed to send termination signal to daemon".to_string(),
                ));
            }
        }

        // Wait for daemon to shut down (with timeout)
        for _ in 0..50 {
            if !is_running() {
                remove_pid_file().map_err(crate::Error::Io)?;
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        Err(crate::Error::Config(
            "Daemon did not shut down gracefully".to_string(),
        ))
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

    #[test]
    fn test_pid_file_path() {
        let path = pid_file_path();
        assert!(path.to_string_lossy().contains("vicaya"));
        assert!(path.to_string_lossy().ends_with("daemon.pid"));
    }

    #[test]
    fn test_write_and_read_pid() {
        let test_pid = 12345;
        write_pid(test_pid).unwrap();

        let read_pid = get_pid();
        assert_eq!(read_pid, Some(test_pid));

        // Cleanup
        remove_pid_file().unwrap();
    }
}
