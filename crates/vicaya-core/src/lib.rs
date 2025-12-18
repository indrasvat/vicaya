//! vicaya-core: Core types, configuration, and logging for vicaya.

pub mod build_info;
pub mod config;
pub mod daemon;
pub mod error;
pub mod filter;
pub mod ipc;
pub mod logging;
pub mod paths;

pub use config::Config;
pub use error::{Error, Result};
