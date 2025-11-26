//! Logging setup for vicaya.

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initialize logging for vicaya.
///
/// Uses `RUST_LOG` environment variable for filtering.
/// Default level: info
pub fn init() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("vicaya=info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}

/// Initialize logging with a custom log level.
pub fn init_with_level(level: &str) {
    let filter = EnvFilter::new(format!("vicaya={level}"));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}
