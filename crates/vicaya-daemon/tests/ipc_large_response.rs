use std::io::{BufReader, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::tempdir;
use vicaya_core::config::PerformanceConfig;
use vicaya_core::ipc::{Request, Response, MAX_IPC_MESSAGE_BYTES};
use vicaya_core::Config;

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

#[test]
fn it_handles_large_search_responses_without_truncation() {
    let vicaya_dir = tempdir().unwrap();
    let root = tempdir().unwrap();

    // Create enough files (with long names) that the SearchResults JSON exceeds typical socket
    // buffer sizes and would be truncated if the daemon wrote on a non-blocking stream.
    for i in 0..400 {
        let name = format!("LargeResponseTest_{i:04}_{}_file.txt", "x".repeat(64));
        std::fs::write(root.path().join(name), "").unwrap();
    }

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
            query: "LargeResponseTest".to_string(),
            limit: 500,
            scope: None,
            recent_if_empty: false,
        },
    );

    match response {
        Response::SearchResults { results } => {
            assert!(
                results.len() >= 300,
                "expected many results, got {}",
                results.len()
            );
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
fn it_rejects_oversized_requests_and_stays_responsive() {
    let vicaya_dir = tempdir().unwrap();
    let root = tempdir().unwrap();

    std::fs::write(root.path().join("healthy.txt"), "").unwrap();

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

    let mut malicious = UnixStream::connect(&socket).expect("Should connect malformed client");
    let oversized = vec![b'a'; MAX_IPC_MESSAGE_BYTES + 1];
    malicious
        .write_all(&oversized)
        .expect("Should write oversized payload");
    malicious
        .shutdown(Shutdown::Write)
        .expect("Should close malformed writer");

    let mut response_reader = BufReader::new(malicious);
    let line = vicaya_core::ipc::read_message(&mut response_reader)
        .expect("Daemon should respond to oversized payload")
        .expect("Daemon should emit an error response");
    let response = Response::from_json(&line).expect("Oversized response should be valid JSON");
    match response {
        Response::Error { message } => {
            assert!(
                message.contains("exceeds"),
                "expected oversize error, got {message}"
            );
        }
        other => panic!("unexpected malformed-client response: {:?}", other),
    }

    let healthy = ipc_request(
        &socket,
        &Request::Search {
            query: "healthy.txt".to_string(),
            limit: 10,
            scope: None,
            recent_if_empty: false,
        },
    );

    match healthy {
        Response::SearchResults { results } => {
            assert!(
                results.iter().any(|r| r.path.ends_with("healthy.txt")),
                "expected daemon to remain responsive after malformed client"
            );
        }
        other => panic!(
            "unexpected healthy response after malformed client: {:?}",
            other
        ),
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
