use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs");

    let repo_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf();

    let git_sha = Command::new("git")
        .args([
            "-C",
            repo_root.to_str().unwrap(),
            "rev-parse",
            "--short",
            "HEAD",
        ])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into());

    let timestamp = chrono::Utc::now().to_rfc3339();

    let target = env::var("TARGET").unwrap_or_else(|_| "unknown".into());

    println!("cargo:rustc-env=VICAYA_BUILD_GIT_SHA={git_sha}");
    println!("cargo:rustc-env=VICAYA_BUILD_TIMESTAMP={timestamp}");
    println!("cargo:rustc-env=VICAYA_BUILD_TARGET={target}");
}
