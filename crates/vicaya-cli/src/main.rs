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
        Some(Commands::Status) => {
            status()?;
        }
        None => {
            println!("vicaya v{}", env!("CARGO_PKG_VERSION"));
            println!("Use --help for usage information");
        }
    }

    Ok(())
}

fn search(query: &str, limit: usize, format: &str) -> Result<()> {
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

fn status() -> Result<()> {
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
            println!("Daemon Status:");
            println!("  Files indexed: {}", indexed_files);
            println!("  Trigrams: {}", trigram_count);
            println!("  String arena size: {} bytes", arena_size);
            if last_updated > 0 {
                println!(
                    "  Last updated: {}",
                    chrono::DateTime::from_timestamp(last_updated, 0)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_default()
                );
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
    let config_content = format!(r#"# vicaya configuration file
# This file was automatically generated by `vicaya init`

# Directories to index
# Add or remove paths as needed
index_roots = [
    "{}",
]

# Directories and patterns to exclude from indexing
exclusions = [
    ".git",
    "node_modules",
    "target",
    ".DS_Store",
    "*.tmp",
]

# Where to store the index file
index_path = "{}"

# Maximum memory to use for indexing (in MB)
max_memory_mb = 512

[performance]
# Number of threads for parallel scanning
scanner_threads = {}
# Hour of day (0-23) to run automatic reconciliation
reconcile_hour = 3
"#,
        home,
        index_dir.display(),
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
    println!("  • Your home directory: {}", home);
    println!();
    println!("📝 What's excluded:");
    println!("  • .git, node_modules, target");
    println!("  • .DS_Store, *.tmp");
    println!();
    println!("Next steps:");
    println!("  1. Edit {} to customize", config_path.display());
    println!("  2. Run: vicaya rebuild");
    println!("  3. Start daemon: vicaya-daemon");
    println!("  4. Search: vicaya search <query>");

    Ok(())
}
