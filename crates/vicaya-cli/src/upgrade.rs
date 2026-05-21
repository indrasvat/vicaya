//! Self-update support for the vicaya CLI.

use std::fs;
use std::io::{self, IsTerminal, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::Args;
use flate2::read::GzDecoder;
use owo_colors::OwoColorize;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use vicaya_core::{Error, Result};

#[cfg(test)]
use std::sync::{Mutex, OnceLock};

const VERSION_MANIFEST_URL: &str = "https://indrasvat.github.io/vicaya/version.json";
const REPO_API_URL: &str = "https://api.github.com/repos/indrasvat/vicaya/releases/latest";
const CACHE_TTL: Duration = Duration::from_secs(60 * 60 * 24);
const CACHE_NOTICE_MAX_AGE: Duration = Duration::from_secs(60 * 60 * 24 * 7);
const BINARY_NAMES: &[&str] = &["vicaya", "vicaya-daemon", "vicaya-tui"];

/// Command-line options for `vicaya upgrade` and the `vicaya update` alias.
#[derive(Debug, Args)]
pub struct UpgradeArgs {
    /// Only check whether an update is available
    #[arg(long)]
    pub check: bool,

    /// Reinstall the latest release even when this version is already current
    #[arg(long)]
    pub force: bool,

    /// Install into this directory instead of the current executable's directory
    #[arg(long, value_name = "DIR")]
    pub install_dir: Option<PathBuf>,

    /// Do not restart the daemon after replacing binaries
    #[arg(long)]
    pub no_restart_daemon: bool,
}

/// Cached result from the last successful update check.
///
/// The cache lets `vicaya --version` show a fast inline notice without waiting
/// for network I/O on the foreground command path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCache {
    checked_at: i64,
    current_version: String,
    latest_version: String,
    tag_name: String,
    html_url: String,
}

#[derive(Debug, Clone)]
struct Release {
    tag_name: String,
    html_url: String,
    version: Version,
    tarball_url: String,
    checksum_url: String,
}

#[derive(Debug, Deserialize)]
struct VersionManifest {
    version: String,
    tag_name: String,
    release_url: String,
    tarball_url: String,
    checksum_url: String,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

/// Check for or install the latest vicaya release.
///
/// The updater fetches release metadata, verifies the release archive checksum,
/// stops the daemon before replacing binaries, and restores or restarts the
/// daemon according to the previous daemon state and CLI flags.
pub fn run(args: UpgradeArgs) -> Result<()> {
    let current = current_version()?;
    let release = fetch_latest_release()?;

    write_cache(&UpdateCache {
        checked_at: now_epoch(),
        current_version: current.to_string(),
        latest_version: release.version.to_string(),
        tag_name: release.tag_name.clone(),
        html_url: release.html_url.clone(),
    });

    if args.check {
        if release.version > current {
            print_update_notice(&current, &release.version);
            println!("{} {}", label("Release:"), cyan(&release.html_url));
        } else {
            println!("{} vicaya is up to date ({current}).", green("✓"));
        }
        return Ok(());
    }

    if release.version <= current && !args.force {
        println!("{} vicaya is up to date ({current}).", green("✓"));
        return Ok(());
    }

    let install_dir = args
        .install_dir
        .map(Ok)
        .unwrap_or_else(default_install_dir)?;
    let daemon_was_running = vicaya_core::daemon::is_running();

    println!(
        "{} vicaya {} -> {}",
        label("Upgrading"),
        cyan(&current.to_string()),
        green(&release.version.to_string())
    );
    println!("{} {}", label("Release:"), cyan(&release.html_url));
    println!(
        "{} {}",
        label("Install directory:"),
        cyan(&install_dir.display().to_string())
    );

    let bundle = download_and_verify(&release)?;
    let unpacked = tempfile::tempdir()?;
    unpack_tarball(&bundle, unpacked.path())?;
    verify_bundle(unpacked.path())?;

    if daemon_was_running {
        println!(
            "{} Stopping daemon before replacing binaries...",
            amber("!")
        );
        vicaya_core::daemon::stop_daemon()?;
    }

    if let Err(err) = install_bundle(unpacked.path(), &install_dir) {
        restart_daemon_after_failed_install(daemon_was_running);
        return Err(err);
    }

    if daemon_was_running && !args.no_restart_daemon {
        println!("{} Restarting daemon...", amber("!"));
        let pid = vicaya_core::daemon::start_daemon()?;
        println!("{} Daemon restarted (PID: {pid}).", green("✓"));
    } else if daemon_was_running {
        println!(
            "{} Daemon left stopped because --no-restart-daemon was set.",
            amber("!")
        );
    }

    println!(
        "{} Upgrade complete. Run `{}` to verify.",
        green("✓"),
        cyan("vicaya --version")
    );
    Ok(())
}

/// Print an upgrade-specific error message with the CLI color theme.
pub fn print_error(err: &Error) {
    eprintln!("{} {}", amber("Upgrade failed:"), err);
}

/// Print a cached update notice, if a fresh cache says a newer version exists.
///
/// This function never performs network I/O and is safe to call from
/// `vicaya --version`.
pub fn print_cached_notice() {
    if update_check_disabled() {
        return;
    }

    let Some(cache) = read_cache() else {
        return;
    };
    let Ok(current) = current_version() else {
        return;
    };
    let Ok(latest) = parse_version(&cache.latest_version) else {
        return;
    };

    if cache.current_version != current.to_string() || latest <= current {
        return;
    }

    let age = now_epoch().saturating_sub(cache.checked_at);
    if age > CACHE_NOTICE_MAX_AGE.as_secs() as i64 {
        return;
    }

    print_update_notice(&current, &latest);
}

/// Start a detached cache refresh process when the local update cache is stale.
///
/// The child command refreshes metadata silently so foreground commands remain
/// fast and non-blocking.
pub fn spawn_background_refresh() {
    if update_check_disabled() || !cache_refresh_due() {
        return;
    }

    let Ok(current_exe) = std::env::current_exe() else {
        return;
    };

    let _ = Command::new(current_exe)
        .arg("__refresh-update-cache")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// Refresh the update cache without printing user-facing output.
///
/// Failures are intentionally ignored because this runs from a best-effort
/// background path.
pub fn refresh_cache_silently() {
    if update_check_disabled() {
        return;
    }

    let Ok(current) = current_version() else {
        return;
    };
    let Ok(release) = fetch_latest_release() else {
        return;
    };

    write_cache(&UpdateCache {
        checked_at: now_epoch(),
        current_version: current.to_string(),
        latest_version: release.version.to_string(),
        tag_name: release.tag_name,
        html_url: release.html_url,
    });
}

fn fetch_latest_release() -> Result<Release> {
    fetch_manifest_release().or_else(|manifest_err| {
        fetch_github_release().map_err(|api_err| {
            Error::Other(format!(
                "Update check failed via Pages manifest ({manifest_err}); GitHub API fallback also failed ({api_err})"
            ))
        })
    })
}

fn fetch_manifest_release() -> Result<Release> {
    let manifest_url = version_manifest_url();
    let manifest: VersionManifest = get_json(&manifest_url)?;
    release_from_manifest(manifest)
}

fn release_from_manifest(manifest: VersionManifest) -> Result<Release> {
    let version = parse_version(&manifest.version)?;

    Ok(Release {
        tag_name: manifest.tag_name,
        html_url: manifest.release_url,
        version,
        tarball_url: manifest.tarball_url,
        checksum_url: manifest.checksum_url,
    })
}

fn fetch_github_release() -> Result<Release> {
    let release: GitHubRelease = get_json(REPO_API_URL)?;
    let version = parse_version(&release.tag_name)?;
    let tarball_name = release_tarball_name();
    let checksum_name = format!("{tarball_name}.sha256");
    let tarball_url = asset_url(&release.assets, tarball_name)?;
    let checksum_url = asset_url(&release.assets, &checksum_name)?;

    Ok(Release {
        tag_name: release.tag_name,
        html_url: release.html_url,
        version,
        tarball_url,
        checksum_url,
    })
}

fn download_and_verify(release: &Release) -> Result<Vec<u8>> {
    println!(
        "{} Downloading {}...",
        label("Fetch:"),
        release_tarball_name()
    );
    let bundle = get_bytes(&release.tarball_url)?;
    let checksum = get_text(&release.checksum_url)?;
    verify_sha256(&bundle, &checksum)?;
    println!("{} Checksum verified.", green("✓"));
    Ok(bundle)
}

fn get_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T> {
    let agent = http_agent();
    let request = apply_auth(
        url,
        agent
            .get(url)
            .header("User-Agent", user_agent())
            .header("Accept", "application/vnd.github+json"),
    );
    let mut response = request.call().map_err(http_error)?;
    response.body_mut().read_json().map_err(http_error)
}

fn get_text(url: &str) -> Result<String> {
    let agent = http_agent();
    let request = apply_auth(url, agent.get(url).header("User-Agent", user_agent()));
    let mut response = request.call().map_err(http_error)?;
    response.body_mut().read_to_string().map_err(http_error)
}

fn get_bytes(url: &str) -> Result<Vec<u8>> {
    let agent = http_agent();
    let request = apply_auth(url, agent.get(url).header("User-Agent", user_agent()));
    let mut response = request.call().map_err(http_error)?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .read_to_end(&mut bytes)
        .map_err(Error::Io)?;
    Ok(bytes)
}

fn verify_sha256(bytes: &[u8], checksum_file: &str) -> Result<()> {
    let expected = checksum_file
        .split_whitespace()
        .next()
        .ok_or_else(|| Error::Other("Release checksum file is empty".to_string()))?;
    let actual = to_hex(&Sha256::digest(bytes));
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(Error::Other(format!(
            "Checksum mismatch for release archive: expected {expected}, got {actual}"
        )));
    }
    Ok(())
}

fn unpack_tarball(bytes: &[u8], output_dir: &Path) -> Result<()> {
    let decoder = GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(decoder);
    for entry in archive.entries().map_err(Error::Io)? {
        let mut entry = entry.map_err(Error::Io)?;
        let unpacked = entry.unpack_in(output_dir).map_err(Error::Io)?;
        if !unpacked {
            return Err(Error::Other(
                "Release archive contains an unsafe path".to_string(),
            ));
        }
    }
    Ok(())
}

fn verify_bundle(bundle_root: &Path) -> Result<()> {
    for binary in BINARY_NAMES {
        let path = bundle_root.join("bin").join(binary);
        let metadata = fs::symlink_metadata(&path).map_err(|err| {
            Error::Other(format!(
                "Release archive is missing {}: {}",
                path.display(),
                err
            ))
        })?;
        if !metadata.file_type().is_file() {
            return Err(Error::Other(format!(
                "Release archive entry is not a regular file: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn install_bundle(bundle_root: &Path, install_dir: &Path) -> Result<()> {
    fs::create_dir_all(install_dir)?;
    let rollback_dir = tempfile::tempdir()?;
    let mut backups: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();

    for binary in BINARY_NAMES {
        let source = bundle_root.join("bin").join(binary);
        let destination = install_dir.join(binary);
        let backup = rollback_dir.path().join(binary);

        if destination.exists() {
            fs::copy(&destination, &backup)?;
            backups.push((destination.clone(), Some(backup)));
        } else {
            backups.push((destination.clone(), None));
        }

        if let Err(err) = replace_binary(&source, &destination) {
            restore_backups(&backups);
            return Err(err);
        }
        println!(
            "{} Installed {}",
            green("✓"),
            cyan(&destination.display().to_string())
        );
    }

    Ok(())
}

fn replace_binary(source: &Path, destination: &Path) -> Result<()> {
    let tmp = destination.with_file_name(format!(
        ".{}.{}.new",
        destination
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("vicaya"),
        std::process::id()
    ));
    fs::copy(source, &tmp)?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))?;
    fs::rename(&tmp, destination)?;
    Ok(())
}

fn restore_backups(backups: &[(PathBuf, Option<PathBuf>)]) {
    for (destination, backup) in backups.iter().rev() {
        match backup {
            Some(backup) => {
                let _ = fs::copy(backup, destination);
                let _ = fs::set_permissions(destination, fs::Permissions::from_mode(0o755));
            }
            None => {
                let _ = fs::remove_file(destination);
            }
        }
    }
}

fn restart_daemon_after_failed_install(daemon_was_running: bool) {
    if !daemon_was_running {
        return;
    }

    eprintln!(
        "{} Attempting to restart daemon after failed install...",
        amber("!")
    );
    match vicaya_core::daemon::start_daemon() {
        Ok(pid) => eprintln!("{} Daemon restarted (PID: {pid}).", green("✓")),
        Err(restart_err) => eprintln!(
            "{} Could not restart daemon after failed install: {}",
            amber("!"),
            restart_err
        ),
    }
}

fn default_install_dir() -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    exe.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| Error::Other("Could not determine current executable directory".to_string()))
}

fn asset_url(assets: &[GitHubAsset], name: &str) -> Result<String> {
    assets
        .iter()
        .find(|asset| asset.name == name)
        .map(|asset| asset.browser_download_url.clone())
        .ok_or_else(|| Error::Other(format!("Release asset not found: {name}")))
}

fn current_version() -> Result<Version> {
    parse_version(vicaya_core::build_info::BUILD_INFO.version)
}

fn parse_version(input: &str) -> Result<Version> {
    Version::parse(input.trim_start_matches('v')).map_err(|err| {
        Error::Other(format!(
            "Could not parse semantic version '{}': {}",
            input, err
        ))
    })
}

fn release_tarball_name() -> &'static str {
    "vicaya-universal.tar.gz"
}

fn cache_refresh_due() -> bool {
    match read_cache() {
        Some(cache) => now_epoch().saturating_sub(cache.checked_at) > CACHE_TTL.as_secs() as i64,
        None => true,
    }
}

fn read_cache() -> Option<UpdateCache> {
    let content = fs::read_to_string(cache_path()).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_cache(cache: &UpdateCache) {
    let path = cache_path();
    let Some(parent) = path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(content) = serde_json::to_string_pretty(cache) else {
        return;
    };
    let _ = fs::write(path, content);
}

fn cache_path() -> PathBuf {
    #[cfg(test)]
    if let Some(path) = test_cache_dir() {
        return path.join("update-check.json");
    }

    vicaya_core::paths::vicaya_dir().join("update-check.json")
}

#[cfg(test)]
static TEST_CACHE_DIR: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

#[cfg(test)]
fn test_cache_dir() -> Option<PathBuf> {
    TEST_CACHE_DIR
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

fn update_check_disabled() -> bool {
    std::env::var_os("VICAYA_NO_UPDATE_CHECK").is_some()
        || matches!(
            std::env::var("VICAYA_UPDATE_CHECK").ok().as_deref(),
            Some("0" | "false" | "off" | "no")
        )
}

fn http_agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .build();
    config.into()
}

fn apply_auth<T>(url: &str, request: ureq::RequestBuilder<T>) -> ureq::RequestBuilder<T> {
    if !url.starts_with("https://api.github.com/") {
        return request;
    }

    match std::env::var("GITHUB_TOKEN")
        .ok()
        .filter(|token| !token.trim().is_empty())
    {
        Some(token) => request.header("Authorization", format!("Bearer {token}")),
        None => request,
    }
}

fn print_update_notice(current: &Version, latest: &Version) {
    println!(
        "{} {} -> {}. Run `{}` or `{}`.",
        amber("Update available:"),
        cyan(&current.to_string()),
        green(&latest.to_string()),
        cyan("vicaya upgrade"),
        cyan("vicaya update")
    );
}

fn label(value: &str) -> String {
    if io::stdout().is_terminal() {
        value.bold().bright_blue().to_string()
    } else {
        value.to_string()
    }
}

fn cyan(value: &str) -> String {
    if io::stdout().is_terminal() {
        value.bright_cyan().to_string()
    } else {
        value.to_string()
    }
}

fn green(value: &str) -> String {
    if io::stdout().is_terminal() {
        value.bright_green().to_string()
    } else {
        value.to_string()
    }
}

fn amber(value: &str) -> String {
    if io::stdout().is_terminal() {
        value.truecolor(245, 158, 11).bold().to_string()
    } else {
        value.to_string()
    }
}

fn to_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

fn user_agent() -> String {
    format!(
        "vicaya/{} (+https://github.com/indrasvat/vicaya)",
        vicaya_core::build_info::BUILD_INFO.version
    )
}

fn version_manifest_url() -> String {
    std::env::var("VICAYA_VERSION_MANIFEST_URL")
        .unwrap_or_else(|_| VERSION_MANIFEST_URL.to_string())
}

fn http_error(err: ureq::Error) -> Error {
    if matches!(err, ureq::Error::StatusCode(403)) {
        return Error::Other(
            "Update check failed: GitHub returned HTTP 403. Set GITHUB_TOKEN for authenticated release checks if you are rate limited."
                .to_string(),
        );
    }
    Error::Other(format!("Update check failed: {err}"))
}

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::GzEncoder, Compression};
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Mutex, OnceLock};
    use tar::{Builder, Header};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn parse_version_accepts_release_tags() {
        assert_eq!(parse_version("v1.2.3").unwrap(), Version::new(1, 2, 3));
        assert_eq!(parse_version("1.2.3").unwrap(), Version::new(1, 2, 3));
    }

    #[test]
    fn verify_sha256_accepts_common_checksum_file_format() {
        let bytes = b"vicaya";
        let checksum = format!(
            "{}  vicaya-universal.tar.gz\n",
            to_hex(&Sha256::digest(bytes))
        );
        verify_sha256(bytes, &checksum).unwrap();
    }

    #[test]
    fn verify_sha256_rejects_mismatches() {
        let err = verify_sha256(b"vicaya", "0000  vicaya-universal.tar.gz").unwrap_err();
        assert!(err.to_string().contains("Checksum mismatch"));
    }

    #[test]
    fn asset_url_finds_exact_asset_name() {
        let assets = vec![GitHubAsset {
            name: "vicaya-universal.tar.gz".to_string(),
            browser_download_url: "https://example.test/vicaya.tar.gz".to_string(),
        }];
        assert_eq!(
            asset_url(&assets, "vicaya-universal.tar.gz").unwrap(),
            "https://example.test/vicaya.tar.gz"
        );
    }

    #[test]
    fn release_from_manifest_uses_static_pages_urls() {
        let release = release_from_manifest(VersionManifest {
            version: "1.3.0".to_string(),
            tag_name: "v1.3.0".to_string(),
            release_url: "https://github.com/indrasvat/vicaya/releases/tag/v1.3.0".to_string(),
            tarball_url: "https://github.com/indrasvat/vicaya/releases/download/v1.3.0/vicaya-universal.tar.gz".to_string(),
            checksum_url: "https://github.com/indrasvat/vicaya/releases/download/v1.3.0/vicaya-universal.tar.gz.sha256".to_string(),
        })
        .unwrap();

        assert_eq!(release.version, Version::new(1, 3, 0));
        assert_eq!(release.tag_name, "v1.3.0");
        assert!(release.tarball_url.ends_with("vicaya-universal.tar.gz"));
        assert!(release.checksum_url.ends_with(".sha256"));
    }

    #[test]
    fn run_check_uses_pages_manifest_and_writes_cache() {
        let _guard = env_lock();
        let vicaya_dir = tempfile::tempdir().unwrap();
        let server = TestServer::new(|base| {
            vec![ResponseRoute::json(
                "/version.json",
                manifest_json(base, "99.0.0"),
            )]
        });
        set_test_env(vicaya_dir.path(), &server.url("/version.json"));

        run(UpgradeArgs {
            check: true,
            force: false,
            install_dir: None,
            no_restart_daemon: false,
        })
        .unwrap();

        let cache = read_cache().unwrap();
        assert_eq!(cache.latest_version, "99.0.0");
        assert_eq!(cache.tag_name, "v99.0.0");
        assert!(cache.html_url.ends_with("/v99.0.0"));
        clear_test_env();
    }

    #[test]
    fn run_force_downloads_verifies_and_installs_bundle() {
        let _guard = env_lock();
        let vicaya_dir = tempfile::tempdir().unwrap();
        let install_dir = tempfile::tempdir().unwrap();
        let bundle = release_bundle();
        let checksum = format!(
            "{}  vicaya-universal.tar.gz\n",
            to_hex(&Sha256::digest(&bundle))
        );
        let server = TestServer::new(|base| {
            vec![
                ResponseRoute::json("/version.json", manifest_json(base, "99.0.0")),
                ResponseRoute::bytes("/vicaya-universal.tar.gz", bundle.clone()),
                ResponseRoute::text("/vicaya-universal.tar.gz.sha256", checksum.clone()),
            ]
        });
        set_test_env(vicaya_dir.path(), &server.url("/version.json"));

        run(UpgradeArgs {
            check: false,
            force: true,
            install_dir: Some(install_dir.path().to_path_buf()),
            no_restart_daemon: true,
        })
        .unwrap();

        for binary in BINARY_NAMES {
            let path = install_dir.path().join(binary);
            let content = fs::read_to_string(&path).unwrap();
            assert!(content.contains(binary));
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o755
            );
        }
        clear_test_env();
    }

    #[test]
    fn install_bundle_rolls_back_replaced_binaries_on_failure() {
        let install_dir = tempfile::tempdir().unwrap();
        let bundle_dir = tempfile::tempdir().unwrap();
        let bin_dir = bundle_dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::write(install_dir.path().join("vicaya"), "old-vicaya").unwrap();
        fs::write(install_dir.path().join("vicaya-daemon"), "old-daemon").unwrap();
        write_executable(&bin_dir.join("vicaya"), "new-vicaya");
        fs::create_dir(bin_dir.join("vicaya-daemon")).unwrap();

        let _err = install_bundle(bundle_dir.path(), install_dir.path()).unwrap_err();

        assert_eq!(
            fs::read_to_string(install_dir.path().join("vicaya")).unwrap(),
            "old-vicaya"
        );
        assert_eq!(
            fs::read_to_string(install_dir.path().join("vicaya-daemon")).unwrap(),
            "old-daemon"
        );
        assert!(!install_dir.path().join("vicaya-tui").exists());
    }

    #[test]
    fn cache_helpers_respect_ttl_and_disable_env() {
        let _guard = env_lock();
        let vicaya_dir = tempfile::tempdir().unwrap();
        set_test_env(vicaya_dir.path(), "http://127.0.0.1/version.json");

        assert!(cache_refresh_due());
        write_cache(&UpdateCache {
            checked_at: now_epoch(),
            current_version: "1.0.0".to_string(),
            latest_version: "1.0.1".to_string(),
            tag_name: "v1.0.1".to_string(),
            html_url: "https://example.test/v1.0.1".to_string(),
        });
        assert!(!cache_refresh_due());
        assert_eq!(read_cache().unwrap().latest_version, "1.0.1");

        std::env::set_var("VICAYA_UPDATE_CHECK", "off");
        assert!(update_check_disabled());
        std::env::remove_var("VICAYA_UPDATE_CHECK");
        std::env::set_var("VICAYA_NO_UPDATE_CHECK", "1");
        assert!(update_check_disabled());
        clear_test_env();
    }

    #[test]
    fn archive_validation_rejects_unsafe_or_incomplete_bundles() {
        let output = tempfile::tempdir().unwrap();
        unpack_tarball(&release_bundle(), output.path()).unwrap();
        verify_bundle(output.path()).unwrap();

        let missing = tempfile::tempdir().unwrap();
        fs::create_dir_all(missing.path().join("bin")).unwrap();
        write_executable(&missing.path().join("bin").join("vicaya"), "vicaya");
        let err = verify_bundle(missing.path()).unwrap_err();
        assert!(err.to_string().contains("missing"));
    }

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn set_test_env(vicaya_dir: &Path, manifest_url: &str) {
        *TEST_CACHE_DIR
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(vicaya_dir.to_path_buf());
        std::env::set_var("VICAYA_VERSION_MANIFEST_URL", manifest_url);
        std::env::remove_var("VICAYA_NO_UPDATE_CHECK");
        std::env::remove_var("VICAYA_UPDATE_CHECK");
    }

    fn clear_test_env() {
        *TEST_CACHE_DIR
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        std::env::remove_var("VICAYA_VERSION_MANIFEST_URL");
        std::env::remove_var("VICAYA_NO_UPDATE_CHECK");
        std::env::remove_var("VICAYA_UPDATE_CHECK");
    }

    fn manifest_json(base: &str, version: &str) -> String {
        format!(
            r#"{{
  "version": "{version}",
  "tag_name": "v{version}",
  "release_url": "{base}/releases/tag/v{version}",
  "tarball_url": "{base}/vicaya-universal.tar.gz",
  "checksum_url": "{base}/vicaya-universal.tar.gz.sha256"
}}"#
        )
    }

    fn release_bundle() -> Vec<u8> {
        let mut archive = Vec::new();
        {
            let encoder = GzEncoder::new(&mut archive, Compression::default());
            let mut builder = Builder::new(encoder);
            for binary in BINARY_NAMES {
                let content = format!("#!/bin/sh\necho {binary}\n");
                let mut header = Header::new_gnu();
                header.set_path(format!("bin/{binary}")).unwrap();
                header.set_size(content.len() as u64);
                header.set_mode(0o755);
                header.set_cksum();
                builder
                    .append(&header, content.as_bytes())
                    .expect("append release binary");
            }
            builder.finish().unwrap();
        }
        archive
    }

    fn write_executable(path: &Path, content: &str) {
        fs::write(path, content).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    struct ResponseRoute {
        path: String,
        content_type: &'static str,
        body: Vec<u8>,
    }

    impl ResponseRoute {
        fn json(path: &str, body: String) -> Self {
            Self {
                path: path.to_string(),
                content_type: "application/json",
                body: body.into_bytes(),
            }
        }

        fn text(path: &str, body: String) -> Self {
            Self {
                path: path.to_string(),
                content_type: "text/plain",
                body: body.into_bytes(),
            }
        }

        fn bytes(path: &str, body: Vec<u8>) -> Self {
            Self {
                path: path.to_string(),
                content_type: "application/octet-stream",
                body,
            }
        }
    }

    struct TestServer {
        base_url: String,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl TestServer {
        fn new(routes: impl FnOnce(&str) -> Vec<ResponseRoute>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let base_url = format!("http://{}", listener.local_addr().unwrap());
            let routes = routes(&base_url);
            let handle = std::thread::spawn(move || {
                for _ in 0..routes.len() {
                    let Ok((stream, _)) = listener.accept() else {
                        return;
                    };
                    handle_request(stream, &routes);
                }
            });
            Self {
                base_url,
                handle: Some(handle),
            }
        }

        fn url(&self, path: &str) -> String {
            format!("{}{}", self.base_url, path)
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    fn handle_request(mut stream: TcpStream, routes: &[ResponseRoute]) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request_line = String::new();
        reader.read_line(&mut request_line).unwrap();
        let path = request_line
            .split_whitespace()
            .nth(1)
            .unwrap_or("/")
            .to_string();
        let route = routes.iter().find(|route| route.path == path);
        let (status, content_type, body) = match route {
            Some(route) => ("200 OK", route.content_type, route.body.as_slice()),
            None => ("404 Not Found", "text/plain", b"not found".as_slice()),
        };
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        stream.write_all(body).unwrap();
    }
}
