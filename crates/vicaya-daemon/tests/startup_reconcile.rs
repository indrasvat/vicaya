use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::tempdir;
use vicaya_core::config::PerformanceConfig;
use vicaya_core::ipc::{Request, Response};
use vicaya_core::Config;
use vicaya_scanner::Scanner;
use vicaya_watcher::IndexUpdate;

struct DaemonChild(Child);

impl Drop for DaemonChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for_socket(socket: &Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if UnixStream::connect(socket).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("Timed out waiting for socket {}", socket.display());
}

fn ipc_request(socket: &Path, req: &Request) -> Response {
    let mut stream = UnixStream::connect(socket).expect("Should connect to socket");

    let mut json = req.to_json().expect("Should serialize request");
    json.push('\n');
    stream
        .write_all(json.as_bytes())
        .expect("Should write request");

    let mut reader = BufReader::new(stream);
    let line = vicaya_core::ipc::read_message(&mut reader)
        .expect("Should read response")
        .expect("Should receive response");
    Response::from_json(&line).expect("Should parse response")
}

fn append_journal_updates(journal: &Path, updates: &[IndexUpdate]) {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(journal)
        .expect("Should open journal");

    for update in updates {
        let json = serde_json::to_string(update).expect("Should serialize update");
        writeln!(file, "{json}").expect("Should append journal update");
    }
}

#[test]
fn it_indexes_offline_changes_via_startup_reconcile() {
    let vicaya_dir = tempdir().unwrap();
    let root = tempdir().unwrap();

    let config = Config {
        index_roots: vec![root.path().to_path_buf()],
        exclusions: vec![],
        index_path: vicaya_dir.path().join("index"),
        max_memory_mb: 128,
        performance: PerformanceConfig {
            scanner_threads: 2,
            reconcile_hour: 3,
        },
    };

    std::fs::create_dir_all(vicaya_dir.path()).unwrap();
    config.save(&vicaya_dir.path().join("config.toml")).unwrap();
    config.ensure_index_dir().unwrap();

    // Build an initial index that does not include "after.txt".
    std::fs::write(root.path().join("before.txt"), "").unwrap();
    let scanner = Scanner::new(config.clone());
    let snapshot = scanner.scan().unwrap();
    snapshot.save(&config.index_path.join("index.bin")).unwrap();

    // Simulate downtime changes: create a new file after the snapshot was produced.
    std::fs::write(root.path().join("after.txt"), "").unwrap();

    let daemon_bin = env!("CARGO_BIN_EXE_vicaya-daemon");
    let mut child = DaemonChild(
        Command::new(daemon_bin)
            .env("VICAYA_DIR", vicaya_dir.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );

    let socket = vicaya_dir.path().join("daemon.sock");
    wait_for_socket(&socket, Duration::from_secs(10));

    // Wait for the startup reconcile to land "after.txt" in the index.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let response = ipc_request(
            &socket,
            &Request::Search {
                query: "after.txt".to_string(),
                limit: 20,
                scope: None,
                filter_scope: None,
                recent_if_empty: false,
            },
        );

        if let Response::SearchResults { results } = response {
            if results.iter().any(|r| r.path.ends_with("after.txt")) {
                break;
            }
        }

        if Instant::now() >= deadline {
            panic!("Timed out waiting for startup reconcile to index after.txt");
        }

        std::thread::sleep(Duration::from_millis(50));
    }

    let _ = ipc_request(&socket, &Request::Shutdown);

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(Some(_)) = child.0.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    panic!("Daemon did not shut down within timeout");
}

#[test]
fn it_replays_journal_when_starting_from_existing_index() {
    let vicaya_dir = tempdir().unwrap();
    let root = tempdir().unwrap();

    let config = Config {
        index_roots: vec![root.path().to_path_buf()],
        exclusions: vec![],
        index_path: vicaya_dir.path().join("index"),
        max_memory_mb: 128,
        performance: PerformanceConfig {
            scanner_threads: 2,
            reconcile_hour: 3,
        },
    };

    std::fs::create_dir_all(vicaya_dir.path()).unwrap();
    config.save(&vicaya_dir.path().join("config.toml")).unwrap();
    config.ensure_index_dir().unwrap();

    std::fs::write(root.path().join("before.txt"), "").unwrap();
    let scanner = Scanner::new(config.clone());
    let snapshot = scanner.scan().unwrap();
    snapshot.save(&config.index_path.join("index.bin")).unwrap();

    let after = root.path().join("after.txt");
    std::fs::write(&after, "").unwrap();
    append_journal_updates(
        &config.index_path.join("index.journal"),
        &[IndexUpdate::Create {
            path: after.to_string_lossy().to_string(),
        }],
    );

    let daemon_bin = env!("CARGO_BIN_EXE_vicaya-daemon");
    let mut child = DaemonChild(
        Command::new(daemon_bin)
            .env("VICAYA_DIR", vicaya_dir.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );

    let socket = vicaya_dir.path().join("daemon.sock");
    wait_for_socket(&socket, Duration::from_secs(10));

    let response = ipc_request(
        &socket,
        &Request::Search {
            query: "after.txt".to_string(),
            limit: 20,
            scope: None,
            filter_scope: None,
            recent_if_empty: false,
        },
    );

    match response {
        Response::SearchResults { results } => {
            assert!(results.iter().any(|r| r.path.ends_with("after.txt")));
        }
        other => panic!("unexpected response: {:?}", other),
    }

    let _ = ipc_request(&socket, &Request::Shutdown);

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(Some(_)) = child.0.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    panic!("Daemon did not shut down within timeout");
}

#[test]
fn it_discards_stale_journal_when_starting_from_fresh_build() {
    let vicaya_dir = tempdir().unwrap();
    let root = tempdir().unwrap();

    let config = Config {
        index_roots: vec![root.path().to_path_buf()],
        exclusions: vec![],
        index_path: vicaya_dir.path().join("index"),
        max_memory_mb: 128,
        performance: PerformanceConfig {
            scanner_threads: 2,
            reconcile_hour: 3,
        },
    };

    std::fs::create_dir_all(vicaya_dir.path()).unwrap();
    config.save(&vicaya_dir.path().join("config.toml")).unwrap();
    config.ensure_index_dir().unwrap();

    let live = root.path().join("live.txt");
    std::fs::write(&live, "").unwrap();
    let journal = config.index_path.join("index.journal");
    append_journal_updates(
        &journal,
        &[IndexUpdate::Delete {
            path: live.to_string_lossy().to_string(),
        }],
    );

    let daemon_bin = env!("CARGO_BIN_EXE_vicaya-daemon");
    let mut child = DaemonChild(
        Command::new(daemon_bin)
            .env("VICAYA_DIR", vicaya_dir.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );

    let socket = vicaya_dir.path().join("daemon.sock");
    wait_for_socket(&socket, Duration::from_secs(10));

    let response = ipc_request(
        &socket,
        &Request::Search {
            query: "live.txt".to_string(),
            limit: 20,
            scope: None,
            filter_scope: None,
            recent_if_empty: false,
        },
    );

    match response {
        Response::SearchResults { results } => {
            assert!(results.iter().any(|r| r.path.ends_with("live.txt")));
        }
        other => panic!("unexpected response: {:?}", other),
    }

    let journal_len = std::fs::metadata(&journal).unwrap().len();
    assert_eq!(journal_len, 0, "fresh build should truncate stale journal");

    let _ = ipc_request(&socket, &Request::Shutdown);

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(Some(_)) = child.0.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    panic!("Daemon did not shut down within timeout");
}
