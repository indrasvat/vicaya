//! vicaya-cli: Command-line interface for vicaya.

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::info;
use vicaya_core::{Config, Result};
use vicaya_scanner::{IndexSnapshot, Scanner};

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
    let config = load_config()?;
    let index_file = config.index_path.join("index.bin");

    if !index_file.exists() {
        eprintln!("Error: Index not found. Run 'vicaya rebuild' first.");
        return Ok(());
    }

    let snapshot = IndexSnapshot::load(&index_file)?;
    let engine = vicaya_index::QueryEngine::new(
        &snapshot.file_table,
        &snapshot.string_arena,
        &snapshot.trigram_index,
    );

    let query_obj = vicaya_index::Query {
        term: query.to_string(),
        limit,
    };

    let results = engine.search(&query_obj);

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
    let config = load_config()?;
    let index_file = config.index_path.join("index.bin");

    if !index_file.exists() {
        println!("Status: No index found");
        println!("Run 'vicaya rebuild' to create an index");
        return Ok(());
    }

    let snapshot = IndexSnapshot::load(&index_file)?;
    let metadata = std::fs::metadata(&index_file)?;

    println!("Index Status:");
    println!("  Files indexed: {}", snapshot.file_table.len());
    println!("  Trigrams: {}", snapshot.trigram_index.trigram_count());
    println!(
        "  String arena size: {} bytes",
        snapshot.string_arena.size()
    );
    println!(
        "  Last modified: {}",
        chrono::DateTime::from_timestamp(
            metadata
                .modified()?
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            0
        )
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default()
    );

    Ok(())
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
