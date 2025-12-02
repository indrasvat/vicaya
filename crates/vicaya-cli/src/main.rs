//! vicaya-cli: Command-line interface for vicaya.

mod ipc_client;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::info;
use vicaya_core::ipc::{Request, Response};
use vicaya_core::{Config, Result};
use vicaya_scanner::Scanner;

use crate::ipc_client::IpcClient;

#[derive(Parser)]
#[command(name = "vicaya")]
#[command(about = "विचय — blazing-fast filesystem search for macOS", long_about = None)]
#[command(version)]
struct Cli {
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
            println!("vicaya v{}", env!("CARGO_PKG_VERSION"));
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
                    "╭─────────────────────────────────────────────────────────╮".bright_blue()
                );
                println!(
                    "{} {} {}",
                    "│".bright_blue(),
                    "विचय (vicaya) — Index Status".bold().bright_white(),
                    "│".bright_blue()
                );
                println!(
                    "{}",
                    "├─────────────────────────────────────────────────────────┤".bright_blue()
                );

                // Daemon info
                println!(
                    "{} {} {}",
                    "│".bright_blue(),
                    format!("  {} Daemon", "●".bright_green()).bright_white(),
                    " │".bright_blue()
                );
                let pid = vicaya_core::daemon::get_pid().unwrap_or(0);
                println!(
                    "{} {}{}{}",
                    "│".bright_blue(),
                    "    PID: ".dimmed(),
                    pid.to_string().bright_cyan(),
                    "                                      │".bright_blue()
                );

                println!(
                    "{}",
                    "├─────────────────────────────────────────────────────────┤".bright_blue()
                );

                // Index stats
                println!(
                    "{} {} {}",
                    "│".bright_blue(),
                    "  Index Statistics".bold().bright_white(),
                    "                            │".bright_blue()
                );

                println!(
                    "{} {}{}{}",
                    "│".bright_blue(),
                    "    Files indexed:    ".dimmed(),
                    format_number(indexed_files).bright_green().bold(),
                    " ".repeat(28 - format_number(indexed_files).len())
                        .to_string()
                        .to_string()
                        + "│"
                );

                println!(
                    "{} {}{}{}",
                    "│".bright_blue(),
                    "    Trigrams:         ".dimmed(),
                    format_number(trigram_count).bright_yellow(),
                    " ".repeat(28 - format_number(trigram_count).len())
                        .to_string()
                        .to_string()
                        + "│"
                );

                let arena_mb = arena_size as f64 / 1_048_576.0;
                println!(
                    "{} {}{}{}",
                    "│".bright_blue(),
                    "    Memory usage:     ".dimmed(),
                    format!("{:.1} MB", arena_mb).bright_magenta(),
                    " ".repeat(28 - format!("{:.1} MB", arena_mb).len())
                        .to_string()
                        .to_string()
                        + "│"
                );

                let index_mb = index_size as f64 / 1_048_576.0;
                println!(
                    "{} {}{}{}",
                    "│".bright_blue(),
                    "    Index file size:  ".dimmed(),
                    format!("{:.1} MB", index_mb).bright_magenta(),
                    " ".repeat(28 - format!("{:.1} MB", index_mb).len())
                        .to_string()
                        .to_string()
                        + "│"
                );

                if last_updated > 0 {
                    let dt = chrono::DateTime::from_timestamp(last_updated, 0)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_default();
                    println!(
                        "{} {}{}{}",
                        "│".bright_blue(),
                        "    Last updated:     ".dimmed(),
                        dt.bright_cyan(),
                        " ".repeat(28 - dt.len()).to_string().to_string() + "│"
                    );
                }

                println!(
                    "{}",
                    "├─────────────────────────────────────────────────────────┤".bright_blue()
                );

                // Efficiency metrics
                println!(
                    "{} {} {}",
                    "│".bright_blue(),
                    "  Efficiency Metrics".bold().bright_white(),
                    "                          │".bright_blue()
                );

                let bytes_per_file = if indexed_files > 0 {
                    arena_size / indexed_files
                } else {
                    0
                };
                println!(
                    "{} {}{}{}",
                    "│".bright_blue(),
                    "    Bytes per file:   ".dimmed(),
                    format!("{} B", bytes_per_file).bright_green(),
                    " ".repeat(28 - format!("{} B", bytes_per_file).len())
                        .to_string()
                        .to_string()
                        + "│"
                );

                let trigrams_per_file = if indexed_files > 0 {
                    trigram_count as f64 / indexed_files as f64
                } else {
                    0.0
                };
                println!(
                    "{} {}{}{}",
                    "│".bright_blue(),
                    "    Trigrams/file:    ".dimmed(),
                    format!("{:.1}", trigrams_per_file).bright_yellow(),
                    " ".repeat(28 - format!("{:.1}", trigrams_per_file).len())
                        .to_string()
                        .to_string()
                        + "│"
                );

                let total_mb = (arena_size + index_size as usize) as f64 / 1_048_576.0;
                let mb_per_kfile = if indexed_files > 0 {
                    total_mb / (indexed_files as f64 / 1000.0)
                } else {
                    0.0
                };
                println!(
                    "{} {}{}{}",
                    "│".bright_blue(),
                    "    Total/1K files:   ".dimmed(),
                    format!("{:.2} MB", mb_per_kfile).bright_magenta(),
                    " ".repeat(28 - format!("{:.2} MB", mb_per_kfile).len())
                        .to_string()
                        .to_string()
                        + "│"
                );

                println!(
                    "{}",
                    "╰─────────────────────────────────────────────────────────╯".bright_blue()
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
