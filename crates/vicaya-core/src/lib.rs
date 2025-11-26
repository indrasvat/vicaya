//! vicaya-core: Core types, configuration, and logging for vicaya.

pub mod config;
pub mod error;
pub mod logging;

pub use config::Config;
pub use error::{Error, Result};
