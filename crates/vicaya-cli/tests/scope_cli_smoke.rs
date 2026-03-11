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

fn daemon_bin_for(vicaya_bin: &Path) -> PathBuf {
    vicaya_bin
        .parent()
        .unwrap()
        .join(format!("vicaya-daemon{}", std::env::consts::EXE_SUFFIX))
}

fn ensure_workspace_daemon_built(vicaya_bin: &Path) {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let daemon_bin = daemon_bin_for(vicaya_bin);

    let status = Command::new("cargo")
        .current_dir(&workspace_root)
        .args(["build", "-p", "vicaya-daemon"])
        .status()
        .unwrap();
    assert!(status.success(), "failed to build vicaya-daemon");
    assert!(
        daemon_bin.exists(),
        "missing daemon binary at {}",
        daemon_bin.display()
    );
}

#[test]
fn cli_search_scope_restricts_results_to_requested_directory() {
    let vicaya_bin = PathBuf::from(env!("CARGO_BIN_EXE_vicaya"));
    let daemon_bin = daemon_bin_for(&vicaya_bin);
    ensure_workspace_daemon_built(&vicaya_bin);

    let vicaya_dir = TempDir::new().unwrap();
    let corpus_root = TempDir::new().unwrap();

    let repo_a = corpus_root.path().join("workspace/repo-a");
    let repo_b = corpus_root.path().join("workspace/repo-b");
    let repo_a_query = repo_a.join("src/query.rs");
    let repo_b_query = repo_b.join("src/query.rs");
    let repo_b_readme = repo_b.join("README.md");

    write_file(&repo_a_query);
    write_file(&repo_b_query);
    write_file(&repo_b_readme);

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

    let _daemon_guard = DaemonGuard {
        vicaya_bin: vicaya_bin.clone(),
        vicaya_dir: vicaya_dir.path().to_path_buf(),
        daemon_bin: daemon_bin.clone(),
    };

    let search = Command::new(&vicaya_bin)
        .env("VICAYA_DIR", vicaya_dir.path())
        .env("VICAYA_DAEMON_BIN", &daemon_bin)
        .args([
            "search",
            "query.rs",
            "--format=json",
            "--limit=20",
            "--scope",
            repo_a.to_string_lossy().as_ref(),
        ])
        .output()
        .unwrap();
    assert!(
        search.status.success(),
        "vicaya search failed: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&search.stdout),
        String::from_utf8_lossy(&search.stderr)
    );

    let results: Vec<serde_json::Value> = serde_json::from_slice(&search.stdout).unwrap();
    let paths: Vec<String> = results
        .iter()
        .filter_map(|v| v.get("path").and_then(|p| p.as_str()).map(str::to_string))
        .collect();

    assert_eq!(
        paths,
        vec![repo_a_query.to_string_lossy().to_string()],
        "expected only repo-a match under explicit scope, got={paths:?}"
    );
}
