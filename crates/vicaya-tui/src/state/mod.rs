//! Application state management.

use crate::client::{DaemonStatus, IpcClient};
use vicaya_index::SearchResult;

/// Application mode
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppMode {
    /// Main search mode
    Search,
    /// Help overlay
    Help,
    /// Confirmation dialog
    Confirm(Action),
}

/// Actions that require confirmation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Quit,
    RebuildIndex,
}

/// Application state
pub struct AppState {
    /// Current mode
    pub mode: AppMode,
    /// Search state
    pub search: SearchState,
    /// UI state
    pub ui: UiState,
    /// IPC client
    pub client: IpcClient,
    /// Daemon status
    pub daemon_status: Option<DaemonStatus>,
    /// Whether to quit
    pub should_quit: bool,
    /// Last error message
    pub error: Option<String>,
}

impl AppState {
    /// Create a new application state
    pub fn new() -> Self {
        let mut client = IpcClient::new();
        let daemon_status = client.status().ok();

        Self {
            mode: AppMode::Search,
            search: SearchState::new(),
            ui: UiState::new(),
            client,
            daemon_status,
            should_quit: false,
            error: None,
        }
    }

    /// Perform a search
    pub fn perform_search(&mut self) {
        let query = self.search.query.trim();
        if query.is_empty() {
            self.search.results.clear();
            self.search.selected_index = 0;
            return;
        }

        self.search.is_searching = true;
        match self.client.search(query, 100) {
            Ok(results) => {
                self.search.set_results(results);
                self.error = None;
            }
            Err(e) => {
                self.error = Some(format!("Search error: {}", e));
                self.search.results.clear();
            }
        }
        self.search.is_searching = false;
    }

    /// Update daemon status
    pub fn update_status(&mut self) {
        self.daemon_status = self.client.status().ok();
    }

    /// Check if should quit
    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// Request quit
    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    /// Toggle help overlay
    pub fn toggle_help(&mut self) {
        self.mode = match self.mode {
            AppMode::Help => AppMode::Search,
            _ => AppMode::Help,
        };
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Search state
pub struct SearchState {
    /// Current query
    pub query: String,
    /// Search results
    pub results: Vec<SearchResult>,
    /// Selected result index
    pub selected_index: usize,
    /// Whether currently searching
    pub is_searching: bool,
    /// Cursor position in query input
    pub cursor_position: usize,
}

impl SearchState {
    /// Create a new search state
    pub fn new() -> Self {
        Self {
            query: String::new(),
            results: Vec::new(),
            selected_index: 0,
            is_searching: false,
            cursor_position: 0,
        }
    }

    /// Update query
    pub fn set_query(&mut self, query: String) {
        self.query = query;
        self.cursor_position = self.query.len();
    }

    /// Clear query
    pub fn clear_query(&mut self) {
        self.query.clear();
        self.cursor_position = 0;
        self.results.clear();
        self.selected_index = 0;
    }

    /// Add character at cursor
    pub fn insert_char(&mut self, c: char) {
        self.query.insert(self.cursor_position, c);
        self.cursor_position += 1;
    }

    /// Remove character before cursor
    pub fn delete_char(&mut self) {
        if self.cursor_position > 0 {
            self.query.remove(self.cursor_position - 1);
            self.cursor_position -= 1;
        }
    }

    /// Move cursor left
    pub fn move_cursor_left(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
        }
    }

    /// Move cursor right
    pub fn move_cursor_right(&mut self) {
        if self.cursor_position < self.query.len() {
            self.cursor_position += 1;
        }
    }

    /// Move selection up
    pub fn select_previous(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        if self.selected_index < self.results.len().saturating_sub(1) {
            self.selected_index += 1;
        }
    }

    /// Jump to top
    pub fn select_first(&mut self) {
        self.selected_index = 0;
    }

    /// Jump to bottom
    pub fn select_last(&mut self) {
        self.selected_index = self.results.len().saturating_sub(1);
    }

    /// Get selected result
    pub fn selected_result(&self) -> Option<&SearchResult> {
        self.results.get(self.selected_index)
    }

    /// Update results
    pub fn set_results(&mut self, results: Vec<SearchResult>) {
        self.results = results;
        // Reset selection if out of bounds
        if self.selected_index >= self.results.len() {
            self.selected_index = self.results.len().saturating_sub(1);
        }
    }
}

impl Default for SearchState {
    fn default() -> Self {
        Self::new()
    }
}

/// UI state
pub struct UiState {
    /// Scroll offset for results list
    pub scroll_offset: usize,
    /// Viewport height
    pub viewport_height: usize,
}

impl UiState {
    /// Create a new UI state
    pub fn new() -> Self {
        Self {
            scroll_offset: 0,
            viewport_height: 0,
        }
    }

    /// Update scroll offset to keep selection visible
    pub fn update_scroll(&mut self, selected_index: usize) {
        // Ensure selected item is visible
        if selected_index < self.scroll_offset {
            self.scroll_offset = selected_index;
        } else if selected_index >= self.scroll_offset + self.viewport_height {
            self.scroll_offset = selected_index.saturating_sub(self.viewport_height - 1);
        }
    }
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}
