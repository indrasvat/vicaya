//! Vicaya TUI - Beautiful terminal UI for fast file search.

use std::path::PathBuf;

use anyhow::Result;
use clap::{ArgAction, Parser};

#[derive(Debug, Parser)]
#[command(name = "vicaya-tui")]
#[command(about = "विचय terminal UI for fast file search", long_about = None)]
#[command(disable_version_flag = true)]
struct Cli {
    /// Show version information and exit
    #[arg(short = 'V', long = "version", action = ArgAction::SetTrue)]
    version: bool,

    /// Start with ksetra scoped to this directory
    scope: Option<PathBuf>,
}

fn parse_startup_scope(scope: Option<PathBuf>) -> Result<Option<PathBuf>> {
    scope
        .map(|path| vicaya_core::paths::resolve_scope_dir(&path))
        .transpose()
        .map_err(anyhow::Error::from)
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.version {
        println!(
            "{}",
            vicaya_core::build_info::BUILD_INFO.version_line("vicaya-tui")
        );
        return Ok(());
    }

    let startup_scope = parse_startup_scope(cli.scope)?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    vicaya_tui::run(startup_scope)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn cli_parses_without_scope() {
        let cli = Cli::parse_from(["vicaya-tui"]);
        assert!(!cli.version);
        assert!(cli.scope.is_none());
    }

    #[test]
    fn cli_parses_relative_scope() {
        let cli = Cli::parse_from(["vicaya-tui", "."]);
        assert_eq!(cli.scope, Some(PathBuf::from(".")));
    }

    #[test]
    fn parse_startup_scope_resolves_relative_directory() {
        let temp = tempfile::tempdir().unwrap();
        let old_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(temp.path()).unwrap();
        let expected_root = std::env::current_dir().unwrap();

        std::fs::create_dir_all("nested").unwrap();
        let resolved = parse_startup_scope(Some(PathBuf::from("./nested")))
            .unwrap()
            .unwrap();

        std::env::set_current_dir(old_cwd).unwrap();

        assert_eq!(resolved, expected_root.join("nested"));
    }

    #[test]
    fn parse_startup_scope_rejects_missing_paths() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing");
        let err = parse_startup_scope(Some(missing)).unwrap_err();
        assert!(
            err.to_string()
                .contains("Failed to resolve scope directory"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_startup_scope_rejects_file_paths() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("note.txt");
        std::fs::write(&file, "").unwrap();

        let err = parse_startup_scope(Some(file)).unwrap_err();
        assert!(
            err.to_string().contains("not a directory"),
            "unexpected error: {err}"
        );
    }
}
