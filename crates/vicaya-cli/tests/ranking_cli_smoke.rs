use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;
use vicaya_core::config::PerformanceConfig;
use vicaya_core::Config;

struct DaemonGuard {
    vicaya_bin: PathBuf,
    vicaya_dir: PathBuf,
    daemon_bin: PathBuf,
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        let _ = Command::new(&self.vicaya_bin)
            .env("VICAYA_DIR", &self.vicaya_dir)
            .env("VICAYA_DAEMON_BIN", &self.daemon_bin)
            .args(["daemon", "stop"])
            .output();
    }
}

fn write_file(path: &Path) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, "").unwrap();
}

#[test]
fn cli_search_returns_json_results() {
    let vicaya_bin = PathBuf::from(env!("CARGO_BIN_EXE_vicaya"));
    let daemon_bin = vicaya_bin
        .parent()
        .unwrap()
        .join(format!("vicaya-daemon{}", std::env::consts::EXE_SUFFIX));
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();

    // Ensure we run the workspace-built daemon binary (not an installed copy).
    // `cargo build` is incremental, so this is cheap after the first run.
    let status = Command::new("cargo")
        .current_dir(&workspace_root)
        .args(["build", "-p", "vicaya-daemon"])
        .status()
        .unwrap();
    assert!(status.success(), "failed to build vicaya-daemon");
    assert!(
        daemon_bin.exists(),
        "vicaya-daemon was not produced at {}",
        daemon_bin.display()
    );

    let vicaya_dir = TempDir::new().unwrap();
    let corpus_root = TempDir::new().unwrap();

    // Create a tiny corpus with both “project” and “cache” lookalikes.
    let project_server = corpus_root
        .path()
        .join("GolandProjects/spartan-ranker/server.go");
    write_file(&project_server);

    write_file(
        &corpus_root
            .path()
            .join("go/pkg/mod/golang.org/x/net@v0.24.0/websocket/server.go"),
    );
    write_file(
        &corpus_root
            .path()
            .join("go/pkg/mod/cloud.google.com/go@v0.34.0/cmd/go/server.go"),
    );

    let doc_invoice = corpus_root.path().join("Documents/invoice_2024.pdf");
    write_file(&doc_invoice);

    write_file(
        &corpus_root
            .path()
            .join("Library/Caches/app/cache/invoice_2024.pdf"),
    );

    let config = Config {
        index_roots: vec![corpus_root.path().to_path_buf()],
        exclusions: Vec::new(),
        index_path: vicaya_dir.path().join("index"),
        max_memory_mb: 64,
        performance: PerformanceConfig {
            scanner_threads: 2,
            reconcile_hour: 3,
        },
    };

    std::fs::create_dir_all(vicaya_dir.path()).unwrap();
    config.save(&vicaya_dir.path().join("config.toml")).unwrap();

    // Build the index.
    let rebuild = Command::new(&vicaya_bin)
        .env("VICAYA_DIR", vicaya_dir.path())
        .env("VICAYA_DAEMON_BIN", &daemon_bin)
        .args(["rebuild"])
        .output()
        .unwrap();
    assert!(
        rebuild.status.success(),
        "vicaya rebuild failed: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&rebuild.stdout),
        String::from_utf8_lossy(&rebuild.stderr)
    );

    // Ensure daemon gets stopped even if assertions below fail.
    let _daemon_guard = DaemonGuard {
        vicaya_bin: vicaya_bin.clone(),
        vicaya_dir: vicaya_dir.path().to_path_buf(),
        daemon_bin: daemon_bin.clone(),
    };

    // Search via CLI JSON output (this may auto-start the daemon).
    let search = Command::new(&vicaya_bin)
        .env("VICAYA_DIR", vicaya_dir.path())
        .env("VICAYA_DAEMON_BIN", &daemon_bin)
        .args(["search", "server.go", "--format=json", "--limit=20"])
        .output()
        .unwrap();
    assert!(
        search.status.success(),
        "vicaya search failed: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&search.stdout),
        String::from_utf8_lossy(&search.stderr)
    );

    let results: Vec<serde_json::Value> = serde_json::from_slice(&search.stdout).unwrap();
    assert!(!results.is_empty(), "expected non-empty JSON results");

    let paths: Vec<String> = results
        .into_iter()
        .filter_map(|v| {
            v.get("path")
                .and_then(|p| p.as_str())
                .map(|s| s.to_string())
        })
        .collect();

    assert!(
        paths.iter().any(|p| p.ends_with("server.go")),
        "expected at least one server.go result. got={paths:?}"
    );

    assert_eq!(
        paths.first().map(|p| p.as_str()),
        Some(project_server.to_string_lossy().as_ref()),
        "expected project server.go to rank first. got={paths:?}"
    );

    assert!(
        paths
            .iter()
            .any(|p| p == project_server.to_string_lossy().as_ref()),
        "expected project server.go in results. got={paths:?}"
    );
}
