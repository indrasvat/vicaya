//! vicaya-cli: Command-line interface for vicaya.

mod ipc_client;

use clap::{ArgAction, Parser, Subcommand};
use std::path::PathBuf;
use tracing::info;
use vicaya_core::ipc::{Request, Response};
use vicaya_core::{Config, Result};
use vicaya_scanner::Scanner;

use crate::ipc_client::IpcClient;

#[derive(Parser)]
#[command(name = "vicaya")]
#[command(about = "विचय — blazing-fast filesystem search for macOS", long_about = None)]
#[command(disable_version_flag = true)]
struct Cli {
    /// Show version information and exit
    #[arg(short = 'V', long = "version", action = ArgAction::SetTrue)]
    version: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize configuration (creates config file if it doesn't exist)
    Init {
        /// Force overwrite existing config
        #[arg(short, long)]
        force: bool,
    },

    /// Search for files
    Search {
        /// Search query
        query: String,

        /// Maximum number of results
        #[arg(short, long, default_value = "20")]
        limit: usize,

        /// Output format (table, json, plain)
        #[arg(short, long, default_value = "table")]
        format: String,
    },

    /// Rebuild the index
    Rebuild {
        /// Dry run (don't actually write)
        #[arg(long)]
        dry_run: bool,
    },

    /// Show index status
    Status {
        /// Output format (pretty, json)
        #[arg(short, long, default_value = "pretty")]
        format: String,
    },

    /// Manage the daemon
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Start the daemon
    Start,
    /// Stop the daemon
    Stop,
    /// Check daemon status
    Status,
}

fn main() -> Result<()> {
    vicaya_core::logging::init();

    let cli = Cli::parse();

    if cli.version {
        println!("{}", vicaya_core::build_info::BUILD_INFO.version_line("vicaya"));
        return Ok(());
    }

    match cli.command {
        Some(Commands::Init { force }) => {
            init_config(force)?;
        }
        Some(Commands::Search {
            query,
            limit,
            format,
        }) => {
            search(&query, limit, &format)?;
        }
        Some(Commands::Rebuild { dry_run }) => {
            rebuild(dry_run)?;
        }
        Some(Commands::Status { format }) => {
            status(&format)?;
        }
        Some(Commands::Daemon { action }) => {
            daemon_command(action)?;
        }
        None => {
            println!("{}", vicaya_core::build_info::BUILD_INFO.version_line("vicaya"));
            println!("Use --help for usage information");
        }
    }

    Ok(())
}

fn search(query: &str, limit: usize, format: &str) -> Result<()> {
    // Auto-start daemon if not running
    if !vicaya_core::daemon::is_running() {
        println!("Daemon is not running. Starting daemon...");
        let pid = vicaya_core::daemon::start_daemon()?;
        println!("✓ Daemon started (PID: {})", pid);

        // Wait a moment for daemon to initialize
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    let mut client = IpcClient::connect()?;

    let request = Request::Search {
        query: query.to_string(),
        limit,
    };

    let response = client.request(&request)?;

    match response {
        Response::SearchResults { results } => {
            match format {
                "json" => {
                    println!("{}", serde_json::to_string_pretty(&results).unwrap());
                }
                "plain" => {
                    for result in results {
                        println!("{}", result.path);
                    }
                }
                _ => {
                    // Table format
                    println!("{:<6} {:<6} {:<20} PATH", "RANK", "SCORE", "MODIFIED");
                    for (i, result) in results.iter().enumerate() {
                        let mtime = chrono::DateTime::from_timestamp(result.mtime, 0)
                            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                            .unwrap_or_default();
                        println!(
                            "{:<6} {:<6.2} {:<20} {}",
                            i + 1,
                            result.score,
                            mtime,
                            result.path
                        );
                    }
                }
            }
            Ok(())
        }
        Response::Error { message } => {
            eprintln!("Error: {}", message);
            Ok(())
        }
        _ => {
            eprintln!("Unexpected response from daemon");
            Ok(())
        }
    }
}

fn rebuild(dry_run: bool) -> Result<()> {
    let config = load_config()?;
    config.ensure_index_dir()?;

    info!("Starting index rebuild...");

    let scanner = Scanner::new(config.clone());
    let snapshot = scanner.scan()?;

    if !dry_run {
        let index_file = config.index_path.join("index.bin");
        snapshot.save(&index_file)?;
        println!("Index rebuilt: {} files", snapshot.file_table.len());
    } else {
        println!("Dry run: would index {} files", snapshot.file_table.len());
    }

    Ok(())
}

fn status(format: &str) -> Result<()> {
    use owo_colors::OwoColorize;

    let mut client = IpcClient::connect()?;
    let request = Request::Status;
    let response = client.request(&request)?;

    match response {
        Response::Status {
            indexed_files,
            trigram_count,
            arena_size,
            last_updated,
        } => {
            if format == "json" {
                // JSON output
                let json = serde_json::json!({
                    "daemon": {
                        "running": true,
                        "pid": vicaya_core::daemon::get_pid().unwrap_or(0),
                    },
                    "index": {
                        "files": indexed_files,
                        "trigrams": trigram_count,
                        "arena_bytes": arena_size,
                        "last_updated": last_updated,
                    },
                    "metrics": {
                        "bytes_per_file": if indexed_files > 0 { arena_size / indexed_files } else { 0 },
                        "trigrams_per_file": if indexed_files > 0 { trigram_count as f64 / indexed_files as f64 } else { 0.0 },
                        "arena_size_mb": arena_size as f64 / 1_048_576.0,
                    }
                });
                println!("{}", serde_json::to_string_pretty(&json).unwrap());
            } else {
                // Pretty output
                let config = load_config()?;
                let index_file = config.index_path.join("index.bin");
                let index_size = std::fs::metadata(&index_file).map(|m| m.len()).unwrap_or(0);

                println!();
                println!(
                    "{}",
                    "╭───────────────────────────────────────────────────────╮".bright_blue()
                );
                println!(
                    "{} {:<53} {}",
                    "│".bright_blue(),
                    "Vicaya — Index Status".bold().bright_white(),
                    "│".bright_blue()
                );
                println!(
                    "{}",
                    "├───────────────────────────────────────────────────────┤".bright_blue()
                );

                // Daemon info
                let pid = vicaya_core::daemon::get_pid().unwrap_or(0);
                // Note: "●" is 3 bytes but 1 char, so use .chars().count() for assertion
                let plain_line = format!("  {} Daemon{:<43}", "●", "");
                assert_eq!(plain_line.chars().count(), 53);
                let daemon_line = format!(" Daemon{:<43}", "");
                println!(
                    "{}   {}{} {}",
                    "│".bright_blue(),
                    "●".bright_green(),
                    daemon_line,
                    "│".bright_blue()
                );

                let pid_str = pid.to_string();
                let plain_line = format!("    PID: {:<44}", pid_str);
                assert_eq!(plain_line.len(), 53);
                let pid_line = format!("{:<44}", pid_str).bright_cyan().to_string();
                println!(
                    "{}     PID: {} {}",
                    "│".bright_blue(),
                    pid_line,
                    "│".bright_blue()
                );

                println!(
                    "{}",
                    "├───────────────────────────────────────────────────────┤".bright_blue()
                );

                // Index stats
                // Build plain line first, then apply colors
                let title_line = format!("{:53}", "  Index Statistics");
                println!(
                    "{} {} {}",
                    "│".bright_blue(),
                    title_line.bold(),
                    "│".bright_blue()
                );

                let files_str = format_number(indexed_files);
                let plain_line = format!("    Files indexed:{:>35}", files_str);
                assert_eq!(plain_line.len(), 53);
                println!(
                    "{} {}{} {}",
                    "│".bright_blue(),
                    "    Files indexed:".dimmed(),
                    format!("{:>35}", files_str).bright_green().bold(),
                    "│".bright_blue()
                );

                let trigrams_str = format_number(trigram_count);
                let plain_line = format!("    Trigrams:{:>40}", trigrams_str);
                assert_eq!(plain_line.len(), 53);
                println!(
                    "{} {}{} {}",
                    "│".bright_blue(),
                    "    Trigrams:".dimmed(),
                    format!("{:>40}", trigrams_str).bright_yellow(),
                    "│".bright_blue()
                );

                let arena_mb = arena_size as f64 / 1_048_576.0;
                let arena_str = format!("{:.1} MB", arena_mb);
                let plain_line = format!("    Memory usage:{:>36}", arena_str);
                assert_eq!(plain_line.len(), 53);
                println!(
                    "{} {}{} {}",
                    "│".bright_blue(),
                    "    Memory usage:".dimmed(),
                    format!("{:>36}", arena_str).bright_magenta(),
                    "│".bright_blue()
                );

                let index_mb = index_size as f64 / 1_048_576.0;
                let index_str = format!("{:.1} MB", index_mb);
                let plain_line = format!("    Index file size:{:>33}", index_str);
                assert_eq!(plain_line.len(), 53);
                println!(
                    "{} {}{} {}",
                    "│".bright_blue(),
                    "    Index file size:".dimmed(),
                    format!("{:>33}", index_str).bright_magenta(),
                    "│".bright_blue()
                );

                if last_updated > 0 {
                    let dt = chrono::DateTime::from_timestamp(last_updated, 0)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_default();
                    let plain_line = format!("    Last updated:{:>36}", dt);
                    assert_eq!(plain_line.len(), 53);
                    println!(
                        "{} {}{} {}",
                        "│".bright_blue(),
                        "    Last updated:".dimmed(),
                        format!("{:>36}", dt).bright_cyan(),
                        "│".bright_blue()
                    );
                }

                println!(
                    "{}",
                    "├───────────────────────────────────────────────────────┤".bright_blue()
                );

                // Efficiency metrics
                let title_line = format!("{:53}", "  Efficiency Metrics");
                println!(
                    "{} {} {}",
                    "│".bright_blue(),
                    title_line.bold().bright_white(),
                    "│".bright_blue()
                );

                let bytes_per_file = if indexed_files > 0 {
                    arena_size / indexed_files
                } else {
                    0
                };
                let bpf_str = format!("{} B", bytes_per_file);
                let plain_line = format!("    Bytes per file:{:>34}", bpf_str);
                assert_eq!(plain_line.len(), 53);
                println!(
                    "{} {}{} {}",
                    "│".bright_blue(),
                    "    Bytes per file:".dimmed(),
                    format!("{:>34}", bpf_str).bright_green(),
                    "│".bright_blue()
                );

                let trigrams_per_file = if indexed_files > 0 {
                    trigram_count as f64 / indexed_files as f64
                } else {
                    0.0
                };
                let tpf_str = format!("{:.1}", trigrams_per_file);
                let plain_line = format!("    Trigrams/file:{:>35}", tpf_str);
                assert_eq!(plain_line.len(), 53);
                println!(
                    "{} {}{} {}",
                    "│".bright_blue(),
                    "    Trigrams/file:".dimmed(),
                    format!("{:>35}", tpf_str).bright_yellow(),
                    "│".bright_blue()
                );

                let total_mb = (arena_size + index_size as usize) as f64 / 1_048_576.0;
                let mb_per_kfile = if indexed_files > 0 {
                    total_mb / (indexed_files as f64 / 1000.0)
                } else {
                    0.0
                };
                let mbpk_str = format!("{:.2} MB", mb_per_kfile);
                let plain_line = format!("    Total/1K files:{:>34}", mbpk_str);
                assert_eq!(plain_line.len(), 53);
                println!(
                    "{} {}{} {}",
                    "│".bright_blue(),
                    "    Total/1K files:".dimmed(),
                    format!("{:>34}", mbpk_str).bright_magenta(),
                    "│".bright_blue()
                );

                println!(
                    "{}",
                    "╰───────────────────────────────────────────────────────╯".bright_blue()
                );
                println!();
            }
            Ok(())
        }
        Response::Error { message } => {
            eprintln!("Error: {}", message);
            Ok(())
        }
        _ => {
            eprintln!("Unexpected response from daemon");
            Ok(())
        }
    }
}

fn format_number(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();

    for (count, c) in s.chars().rev().enumerate() {
        if count > 0 && count % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }

    result.chars().rev().collect()
}

fn daemon_command(action: DaemonAction) -> Result<()> {
    match action {
        DaemonAction::Start => {
            println!("Starting vicaya daemon...");

            if vicaya_core::daemon::is_running() {
                println!("✓ Daemon is already running");
                return Ok(());
            }

            match vicaya_core::daemon::start_daemon() {
                Ok(pid) => {
                    println!("✓ Daemon started successfully (PID: {})", pid);
                    println!("  Socket: {}", vicaya_core::ipc::socket_path().display());
                    Ok(())
                }
                Err(e) => {
                    eprintln!("✗ Failed to start daemon: {}", e);
                    Err(e)
                }
            }
        }
        DaemonAction::Stop => {
            println!("Stopping vicaya daemon...");

            if !vicaya_core::daemon::is_running() {
                println!("Daemon is not running");
                return Ok(());
            }

            match vicaya_core::daemon::stop_daemon() {
                Ok(_) => {
                    println!("✓ Daemon stopped successfully");
                    Ok(())
                }
                Err(e) => {
                    eprintln!("✗ Failed to stop daemon: {}", e);
                    Err(e)
                }
            }
        }
        DaemonAction::Status => {
            if vicaya_core::daemon::is_running() {
                let pid = vicaya_core::daemon::get_pid().unwrap_or(0);
                println!("✓ Daemon is running (PID: {})", pid);
                println!("  Socket: {}", vicaya_core::ipc::socket_path().display());
                println!(
                    "  PID file: {}",
                    vicaya_core::daemon::pid_file_path().display()
                );

                // Try to get detailed status from daemon
                if let Ok(mut client) = IpcClient::connect() {
                    let request = Request::Status;
                    if let Ok(Response::Status {
                        indexed_files,
                        trigram_count,
                        arena_size,
                        last_updated,
                    }) = client.request(&request)
                    {
                        println!("\nIndex Status:");
                        println!("  Files indexed: {}", indexed_files);
                        println!("  Trigrams: {}", trigram_count);
                        println!("  Arena size: {} bytes", arena_size);
                        if last_updated > 0 {
                            println!(
                                "  Last updated: {}",
                                chrono::DateTime::from_timestamp(last_updated, 0)
                                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                                    .unwrap_or_default()
                            );
                        }
                    }
                }
            } else {
                println!("✗ Daemon is not running");
                println!("\nTo start the daemon, run:");
                println!("  vicaya daemon start");
            }
            Ok(())
        }
    }
}

fn load_config() -> Result<Config> {
    let config_path = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()))
        .join("Library")
        .join("Application Support")
        .join("vicaya")
        .join("config.toml");

    if config_path.exists() {
        Config::load(&config_path)
    } else {
        Ok(Config::default())
    }
}

fn init_config(force: bool) -> Result<()> {
    use std::fs;

    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let config_dir = PathBuf::from(&home)
        .join("Library")
        .join("Application Support")
        .join("vicaya");

    let config_path = config_dir.join("config.toml");
    let index_dir = PathBuf::from(&home)
        .join("Library")
        .join("Application Support")
        .join("vicaya")
        .join("index");

    // Check if config already exists
    if config_path.exists() && !force {
        println!("✓ Config already exists at: {}", config_path.display());
        println!("  Use --force to overwrite");
        return Ok(());
    }

    // Create directories
    fs::create_dir_all(&config_dir)?;
    fs::create_dir_all(&index_dir)?;

    // Create default config with user's home directory
    let config_content = format!(
        r#"# vicaya configuration file
# This file was automatically generated by `vicaya init`

# Directories to index
# Add or remove paths as needed
index_roots = [
    "~/",
]

# Directories and patterns to exclude from indexing
# Organized by category for easy customization
exclusions = [
    # Version control
    ".git",
    ".svn",
    ".hg",

    # Build outputs
    "target",           # Rust
    "build",
    "dist",
    "out",
    "bin",
    "obj",

    # Dependencies
    "node_modules",     # JavaScript
    "vendor",           # Go, Ruby, PHP
    ".cargo",           # Rust dependencies cache
    ".rustup",          # Rust toolchain

    # Python
    "__pycache__",
    "*.pyc",
    "*.pyo",
    ".venv",
    "venv",
    "env",
    ".pytest_cache",
    ".tox",
    ".mypy_cache",

    # JavaScript/Node
    ".npm",
    ".yarn",
    ".pnp",
    ".next",
    ".nuxt",

    # Java/JVM
    ".gradle",
    ".m2",
    ".ivy2",

    # IDEs and editors
    ".idea",
    ".vscode",
    ".vs",
    "*.swp",
    "*.swo",
    "*~",
    ".project",
    ".classpath",

    # macOS
    ".DS_Store",
    ".AppleDouble",
    ".LSOverride",
    "._*",

    # Cache and temp
    ".cache",
    "*.tmp",
    "*.temp",
    ".thumbs",
    "Thumbs.db",

    # Logs
    "*.log",
    "logs",

    # Test coverage
    "coverage",
    ".coverage",
    "htmlcov",
    ".nyc_output",
]

# Where to store the index file
index_path = "~/Library/Application Support/vicaya/index"

# Maximum memory to use for indexing (in MB)
max_memory_mb = 512

[performance]
# Number of threads for parallel scanning
scanner_threads = {}
# Hour of day (0-23) to run automatic reconciliation
reconcile_hour = 3
"#,
        num_cpus::get().max(2)
    );

    // Write config file
    fs::write(&config_path, config_content)?;

    println!("✓ Configuration initialized successfully!");
    println!();
    println!("Config file: {}", config_path.display());
    println!("Index location: {}", index_dir.display());
    println!();
    println!("📝 What's indexed:");
    println!("  • Your home directory: ~/");
    println!();
    println!("📝 What's excluded (60+ patterns):");
    println!("  • Version control: .git, .svn, .hg");
    println!("  • Build outputs: target, build, dist, out");
    println!("  • Dependencies: node_modules, vendor, .cargo");
    println!("  • Python: __pycache__, .venv, *.pyc");
    println!("  • IDEs: .idea, .vscode, .vs");
    println!("  • Cache/temp: .cache, *.tmp, *.log");
    println!("  • macOS: .DS_Store, ._*");
    println!();
    println!("Next steps:");
    println!("  1. Edit {} to customize", config_path.display());
    println!("  2. Run: vicaya rebuild");
    println!("  3. Start daemon: vicaya-daemon");
    println!("  4. Search: vicaya search <query>");

    Ok(())
}
