use std::fs;
use std::path::Path;

fn main() {
    // Auto-install pre-push hook for developers
    // This runs automatically when they build the project

    let hook_source = Path::new("../../.cargo-husky/hooks/pre-push");
    let hook_dest = Path::new("../../.git/hooks/pre-push");

    // Only install if we're in a git repo and the hook exists
    if hook_source.exists() && hook_dest.parent().map(|p| p.exists()).unwrap_or(false) {
        if let Ok(content) = fs::read(hook_source) {
            if fs::write(hook_dest, &content).is_ok() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(metadata) = fs::metadata(hook_dest) {
                        let mut perms = metadata.permissions();
                        perms.set_mode(0o755); // Make executable
                        let _ = fs::set_permissions(hook_dest, perms);
                    }
                }
                println!("cargo:warning=✅ Pre-push hook installed to .git/hooks/pre-push");
            }
        }
    }

    // Re-run if the hook source changes
    println!("cargo:rerun-if-changed=../../.cargo-husky/hooks/pre-push");
}
