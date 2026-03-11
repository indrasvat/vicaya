use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::tempdir;
use vicaya_core::config::PerformanceConfig;
use vicaya_core::ipc::{Request, Response};
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
fn daemon_search_filter_scope_restricts_results_before_limit() {
    let vicaya_dir = tempdir().unwrap();
    let root = tempdir().unwrap();

    let repo_a = root.path().join("workspace/repo-a/src");
    let repo_b = root.path().join("workspace/repo-b/src");
    std::fs::create_dir_all(&repo_a).unwrap();
    std::fs::create_dir_all(&repo_b).unwrap();
    std::fs::write(repo_a.join("query.rs"), "").unwrap();
    std::fs::write(repo_b.join("query.rs"), "").unwrap();

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
            query: "query.rs".to_string(),
            limit: 10,
            scope: Some(repo_a.parent().unwrap().to_string_lossy().to_string()),
            filter_scope: Some(repo_a.parent().unwrap().to_string_lossy().to_string()),
            recent_if_empty: false,
        },
    );

    match response {
        Response::SearchResults { results } => {
            assert_eq!(results.len(), 1, "expected a single scoped result");
            assert_eq!(
                results[0].path,
                repo_a.join("query.rs").to_string_lossy().to_string()
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
