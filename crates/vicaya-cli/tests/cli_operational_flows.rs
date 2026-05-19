use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

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

fn daemon_bin_for(vicaya_bin: &Path) -> PathBuf {
    vicaya_bin
        .parent()
        .unwrap()
        .join(format!("vicaya-daemon{}", std::env::consts::EXE_SUFFIX))
}

fn ensure_workspace_daemon_built(vicaya_bin: &Path) -> PathBuf {
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
    daemon_bin
}

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

fn write_config(vicaya_dir: &Path, root: &Path) {
    let config = Config {
        index_roots: vec![root.to_path_buf()],
        exclusions: vec!["target".to_string(), "*.profraw".to_string()],
        index_path: vicaya_dir.join("index"),
        max_memory_mb: 64,
        performance: PerformanceConfig {
            scanner_threads: 2,
            reconcile_hour: 3,
        },
    };
    std::fs::create_dir_all(vicaya_dir).unwrap();
    config.save(&vicaya_dir.join("config.toml")).unwrap();
}

fn run_vicaya(vicaya_bin: &Path, vicaya_dir: &Path, daemon_bin: &Path, args: &[&str]) -> String {
    let output = Command::new(vicaya_bin)
        .env("VICAYA_DIR", vicaya_dir)
        .env("VICAYA_DAEMON_BIN", daemon_bin)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "vicaya {:?} failed: stdout={} stderr={}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn wait_for_status_json(
    vicaya_bin: &Path,
    vicaya_dir: &Path,
    daemon_bin: &Path,
) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let output = Command::new(vicaya_bin)
            .env("VICAYA_DIR", vicaya_dir)
            .env("VICAYA_DAEMON_BIN", daemon_bin)
            .args(["status", "--format=json"])
            .output()
            .unwrap();
        if output.status.success() {
            if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
                return json;
            }
        }
        if Instant::now() >= deadline {
            panic!(
                "timed out waiting for status json: stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn init_version_and_no_command_are_stable() {
    let vicaya_bin = PathBuf::from(env!("CARGO_BIN_EXE_vicaya"));
    let daemon_bin = daemon_bin_for(&vicaya_bin);
    let vicaya_dir = TempDir::new().unwrap();

    let version = run_vicaya(&vicaya_bin, vicaya_dir.path(), &daemon_bin, &["--version"]);
    assert!(version.contains("vicaya"));

    let no_command = run_vicaya(&vicaya_bin, vicaya_dir.path(), &daemon_bin, &[]);
    assert!(no_command.contains("Use --help"));

    let init = run_vicaya(&vicaya_bin, vicaya_dir.path(), &daemon_bin, &["init"]);
    assert!(init.contains("Configuration initialized successfully"));
    assert!(vicaya_dir.path().join("config.toml").exists());

    let repeat = run_vicaya(&vicaya_bin, vicaya_dir.path(), &daemon_bin, &["init"]);
    assert!(repeat.contains("Config already exists"));

    let forced = run_vicaya(
        &vicaya_bin,
        vicaya_dir.path(),
        &daemon_bin,
        &["init", "--force"],
    );
    assert!(forced.contains("Configuration initialized successfully"));
}

#[test]
fn daemon_backed_status_metrics_rebuild_and_search_formats_work_together() {
    let vicaya_bin = PathBuf::from(env!("CARGO_BIN_EXE_vicaya"));
    let daemon_bin = ensure_workspace_daemon_built(&vicaya_bin);
    let vicaya_dir = TempDir::new().unwrap();
    let corpus = TempDir::new().unwrap();

    let repo = corpus.path().join("repo");
    let readme = repo.join("README.md");
    let cargo = repo.join("Cargo.toml");
    let target = repo.join("target").join("ignored.txt");
    write_file(&readme, "# demo\n");
    write_file(&cargo, "[package]\nname = \"demo\"\n");
    write_file(&target, "ignored\n");
    write_config(vicaya_dir.path(), corpus.path());

    let status_before = run_vicaya(
        &vicaya_bin,
        vicaya_dir.path(),
        &daemon_bin,
        &["daemon", "status"],
    );
    assert!(status_before.contains("Daemon is not running"));

    let dry_run = run_vicaya(
        &vicaya_bin,
        vicaya_dir.path(),
        &daemon_bin,
        &["rebuild", "--dry-run"],
    );
    assert!(dry_run.contains("Dry run: would index"));

    let rebuild = run_vicaya(&vicaya_bin, vicaya_dir.path(), &daemon_bin, &["rebuild"]);
    assert!(rebuild.contains("Index rebuilt:"));

    let _guard = DaemonGuard {
        vicaya_bin: vicaya_bin.clone(),
        vicaya_dir: vicaya_dir.path().to_path_buf(),
        daemon_bin: daemon_bin.clone(),
    };

    let table = run_vicaya(
        &vicaya_bin,
        vicaya_dir.path(),
        &daemon_bin,
        &["search", "Cargo.toml", "--limit=5"],
    );
    assert!(table.contains("RANK"));
    assert!(table.contains("Cargo.toml"));

    let plain = run_vicaya(
        &vicaya_bin,
        vicaya_dir.path(),
        &daemon_bin,
        &["search", "README", "--format=plain", "--limit=5"],
    );
    assert!(plain.lines().any(|line| line.ends_with("README.md")));

    let json = run_vicaya(
        &vicaya_bin,
        vicaya_dir.path(),
        &daemon_bin,
        &["search", "Cargo.toml", "--format=json", "--limit=5"],
    );
    let results: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
    assert_eq!(
        results
            .first()
            .and_then(|v| v.get("path"))
            .and_then(|p| p.as_str()),
        Some(cargo.to_string_lossy().as_ref())
    );

    let status_json = wait_for_status_json(&vicaya_bin, vicaya_dir.path(), &daemon_bin);
    assert_eq!(status_json["daemon"]["running"], true);
    assert!(status_json["index"]["files"].as_u64().unwrap() >= 2);

    let pretty_status = run_vicaya(&vicaya_bin, vicaya_dir.path(), &daemon_bin, &["status"]);
    assert!(pretty_status.contains("Vicaya"));
    assert!(pretty_status.contains("Index Statistics"));

    let daemon_status = run_vicaya(
        &vicaya_bin,
        vicaya_dir.path(),
        &daemon_bin,
        &["daemon", "status"],
    );
    assert!(daemon_status.contains("Daemon is running"));
    assert!(daemon_status.contains("Index Status"));

    let metrics_json = run_vicaya(
        &vicaya_bin,
        vicaya_dir.path(),
        &daemon_bin,
        &["metrics", "--format=json", "--no-vmmap"],
    );
    let metrics: serde_json::Value = serde_json::from_str(&metrics_json).unwrap();
    assert_eq!(metrics["schema_version"], 1);
    assert_eq!(metrics["daemon"]["running"], true);

    let metrics_pretty = run_vicaya(
        &vicaya_bin,
        vicaya_dir.path(),
        &daemon_bin,
        &["metrics", "--no-vmmap"],
    );
    assert!(metrics_pretty.contains("Vicaya"));

    let queries = corpus.path().join("bench-queries.txt");
    std::fs::write(&queries, "Cargo.toml\nREADME\n").unwrap();

    let bench_json = run_vicaya(
        &vicaya_bin,
        vicaya_dir.path(),
        &daemon_bin,
        &[
            "metrics",
            "bench",
            "--format=json",
            "--queries",
            queries.to_string_lossy().as_ref(),
            "--runs=3",
            "--warmup=1",
            "--limit=5",
        ],
    );
    let bench: serde_json::Value = serde_json::from_str(&bench_json).unwrap();
    assert_eq!(bench["schema_version"], 1);
    assert_eq!(bench["params"]["query_count"], 2);
    assert_eq!(bench["summary"]["ok_runs"], 3);

    let bench_pretty = run_vicaya(
        &vicaya_bin,
        vicaya_dir.path(),
        &daemon_bin,
        &[
            "metrics",
            "bench",
            "--queries",
            queries.to_string_lossy().as_ref(),
            "--runs=2",
            "--warmup=1",
            "--limit=5",
        ],
    );
    assert!(bench_pretty.contains("Vicaya"));
    assert!(bench_pretty.contains("Runs: 2"));
}
