use std::env;
use std::fs;
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

    install_pre_push_hook(&repo_root);

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

fn install_pre_push_hook(repo_root: &PathBuf) {
    let hook_source = repo_root.join(".cargo-husky/hooks/pre-push");
    let hook_dest = repo_root.join(".git/hooks/pre-push");

    println!(
        "cargo:rerun-if-changed={}",
        hook_source.to_string_lossy()
    );

    if !hook_source.exists() {
        return;
    }

    if !hook_dest
        .parent()
        .map(|p| p.exists())
        .unwrap_or(false)
    {
        return;
    }

    let copy_result = fs::copy(&hook_source, &hook_dest);
    if copy_result.is_err() {
        println!(
            "cargo:warning=Unable to install pre-push hook from {}",
            hook_source.display()
        );
        return;
    }

    #[cfg(unix)]
    if let Ok(metadata) = fs::metadata(&hook_dest) {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = metadata.permissions();
        perms.set_mode(0o755);
        let _ = fs::set_permissions(&hook_dest, perms);
    }

    println!(
        "cargo:warning=✅ Installed pre-push hook from {}",
        hook_source.display()
    );
}
