//! Vicaya TUI - Beautiful terminal UI for fast file search.

use anyhow::Result;

fn main() -> Result<()> {
    if std::env::args().any(|arg| arg == "--version" || arg == "-V") {
        println!(
            "{}",
            vicaya_core::build_info::BUILD_INFO.version_line("vicaya-tui")
        );
        return Ok(());
    }

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    // Run the TUI
    vicaya_tui::run()
}
