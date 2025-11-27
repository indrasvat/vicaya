//! Integration tests for daemon lifecycle management.
//!
//! Note: These tests use a shared mutex to ensure serial execution since they
//! all operate on the same PID file. This prevents race conditions.

use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use vicaya_core::daemon;

// Global mutex to serialize tests that touch the PID file
static TEST_MUTEX: Mutex<()> = Mutex::new(());

/// Test basic daemon lifecycle: write PID, check running, read PID, cleanup.
#[test]
fn test_daemon_pid_lifecycle() {
    let _lock = TEST_MUTEX.lock().unwrap();

    // Clean up any existing PID file first
    let _ = daemon::remove_pid_file();
    // Give filesystem time to sync
    thread::sleep(Duration::from_millis(50));

    // Initially, daemon should not be running
    assert!(
        !daemon::is_running(),
        "Daemon should not be running initially"
    );
    assert_eq!(daemon::get_pid(), None, "PID should be None initially");

    // Write a test PID
    let test_pid = 99999; // Use a PID that's unlikely to exist
    daemon::write_pid(test_pid).expect("Should write PID successfully");
    // Give filesystem time to sync
    thread::sleep(Duration::from_millis(50));

    // The daemon won't actually be "running" since this is just a test PID
    // But we can verify the PID file operations work
    let read_pid = daemon::get_pid();
    assert_eq!(
        read_pid,
        Some(test_pid),
        "Should read back the same PID that was written"
    );

    // Clean up
    daemon::remove_pid_file().expect("Should remove PID file successfully");
    thread::sleep(Duration::from_millis(50));
    assert_eq!(daemon::get_pid(), None, "PID should be None after cleanup");
}

/// Test that is_running() correctly identifies a non-existent process.
#[test]
fn test_is_running_with_invalid_pid() {
    let _lock = TEST_MUTEX.lock().unwrap();

    // Clean up any existing PID file first
    let _ = daemon::remove_pid_file();

    // Write a PID that definitely doesn't exist (very high number)
    let invalid_pid = 2147483647; // Max i32 value
    daemon::write_pid(invalid_pid).expect("Should write PID");

    // is_running should return false for a non-existent process
    assert!(
        !daemon::is_running(),
        "is_running() should return false for non-existent process"
    );

    // Clean up
    daemon::remove_pid_file().expect("Should remove PID file");
}

/// Test multiple write/read cycles.
#[test]
fn test_multiple_pid_write_cycles() {
    let _lock = TEST_MUTEX.lock().unwrap();

    // Clean up any existing PID file first
    let _ = daemon::remove_pid_file();
    thread::sleep(Duration::from_millis(50));

    // Test multiple write/read cycles
    for i in 1..=5 {
        let test_pid = 10000 + i;
        daemon::write_pid(test_pid).expect("Should write PID");
        thread::sleep(Duration::from_millis(50));

        let read_pid = daemon::get_pid();
        assert_eq!(
            read_pid,
            Some(test_pid),
            "Should read back PID {} in cycle {}",
            test_pid,
            i
        );
    }

    // Clean up
    daemon::remove_pid_file().expect("Should remove PID file");
}

/// Test PID file path generation.
#[test]
fn test_pid_file_path_format() {
    let path = daemon::pid_file_path();
    let path_str = path.to_string_lossy();

    assert!(
        path_str.contains("vicaya"),
        "PID file path should contain 'vicaya'"
    );
    assert!(
        path_str.ends_with("daemon.pid"),
        "PID file path should end with 'daemon.pid'"
    );
}

/// Test removal of non-existent PID file (should not error).
#[test]
fn test_remove_nonexistent_pid_file() {
    // Ensure PID file doesn't exist
    let _ = daemon::remove_pid_file();

    // Removing again should not error
    let result = daemon::remove_pid_file();
    assert!(
        result.is_ok(),
        "Removing non-existent PID file should not error"
    );
}

/// Test that get_pid returns None when PID file doesn't exist.
#[test]
fn test_get_pid_no_file() {
    let _lock = TEST_MUTEX.lock().unwrap();

    // Clean up any existing PID file
    let _ = daemon::remove_pid_file();

    let pid = daemon::get_pid();
    assert_eq!(
        pid, None,
        "get_pid() should return None when file doesn't exist"
    );
}

/// Test that get_pid handles corrupted PID file gracefully.
#[test]
fn test_get_pid_corrupted_file() {
    use std::fs;

    let _lock = TEST_MUTEX.lock().unwrap();

    // Clean up any existing PID file
    let _ = daemon::remove_pid_file();

    // Write invalid content to PID file
    let pid_file = daemon::pid_file_path();
    if let Some(parent) = pid_file.parent() {
        fs::create_dir_all(parent).expect("Should create parent directory");
    }
    fs::write(&pid_file, "not-a-number").expect("Should write invalid content");

    // get_pid should return None for corrupted file
    let pid = daemon::get_pid();
    assert_eq!(
        pid, None,
        "get_pid() should return None for corrupted PID file"
    );

    // Clean up
    let _ = daemon::remove_pid_file();
}
