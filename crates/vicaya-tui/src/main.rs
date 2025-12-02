//! Vicaya TUI - Beautiful terminal UI for fast file search.

use anyhow::Result;

fn main() -> Result<()> {
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
