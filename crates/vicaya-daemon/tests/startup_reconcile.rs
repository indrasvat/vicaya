use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::tempdir;
use vicaya_core::config::PerformanceConfig;
use vicaya_core::ipc::{Request, Response};
use vicaya_core::Config;
use vicaya_scanner::Scanner;

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
    let mut line = String::new();
    reader.read_line(&mut line).expect("Should read response");
    Response::from_json(&line).expect("Should parse response")
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
