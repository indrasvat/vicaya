//! vicaya-tui: Beautiful terminal UI for vicaya file search.

pub mod app;
pub mod client;
pub mod state;
pub mod ui;
mod worker;

pub use app::run;
pub use client::IpcClient;
pub use state::{AppMode, AppState};
