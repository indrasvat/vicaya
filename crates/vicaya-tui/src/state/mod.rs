//! Application state management.

use crate::client::DaemonStatus;
use vicaya_index::SearchResult;

/// Application mode
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppMode {
    /// Main search mode
    Search,
    /// Help overlay
    Help,
    /// Drishti (view) switcher overlay
    DrishtiSwitcher,
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
    /// Current Drishti (view)
    pub view: ViewKind,
    /// Search state
    pub search: SearchState,
    /// Preview state
    pub preview: PreviewState,
    /// UI state
    pub ui: UiState,
    /// Daemon status
    pub daemon_status: Option<DaemonStatus>,
    /// Whether to quit
    pub should_quit: bool,
    /// Last error message
    pub error: Option<String>,
    /// Path to print on exit (for terminal integration)
    pub print_on_exit: Option<String>,
    /// Path to open in editor after exit
    pub open_in_editor: Option<String>,
}

impl AppState {
    /// Create a new application state
    pub fn new() -> Self {
        Self {
            mode: AppMode::Search,
            view: ViewKind::Patra,
            search: SearchState::new(),
            preview: PreviewState::new(),
            ui: UiState::new(),
            daemon_status: None,
            should_quit: false,
            error: None,
            print_on_exit: None,
            open_in_editor: None,
        }
    }

    /// Perform a search
    pub fn clear_results(&mut self) {
        self.search.results.clear();
        self.search.selected_index = 0;
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

    /// Toggle drishti switcher overlay.
    pub fn toggle_drishti_switcher(&mut self) {
        self.mode = match self.mode {
            AppMode::DrishtiSwitcher => AppMode::Search,
            _ => AppMode::DrishtiSwitcher,
        };
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Focus target in search mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusTarget {
    /// Search input is focused
    Input,
    /// Results list is focused
    Results,
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
    /// Current focus target
    pub focus: FocusTarget,
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
            focus: FocusTarget::Input,
        }
    }

    /// Toggle focus between input and results
    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            FocusTarget::Input => FocusTarget::Results,
            FocusTarget::Results => FocusTarget::Input,
        };
    }

    /// Check if input is focused
    pub fn is_input_focused(&self) -> bool {
        self.focus == FocusTarget::Input
    }

    /// Check if results are focused
    pub fn is_results_focused(&self) -> bool {
        self.focus == FocusTarget::Results
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
    /// Drishti switcher state
    pub drishti_switcher: DrishtiSwitcherState,
}

impl UiState {
    /// Create a new UI state
    pub fn new() -> Self {
        Self {
            scroll_offset: 0,
            viewport_height: 0,
            drishti_switcher: DrishtiSwitcherState::new(),
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

/// Drishti (view) in the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewKind {
    /// `Patra` — Files
    Patra,
    /// `Sthana` — Directories
    Sthana,
    /// `Smriti` — Recent / history
    Smriti,
    /// `Navatama` — Recently modified
    Navatama,
    /// `Brihat` — Large files
    Brihat,
    /// `Antarvicaya` — Content (grep)
    Antarvicaya,
    /// `Sanketa` — Symbols
    Sanketa,
    /// `Itihasa` — Git
    Itihasa,
    /// `Parivartana` — Changed since…
    Parivartana,
    /// `Sambandha` — Related
    Sambandha,
    /// `Ankita` — Pinned
    Ankita,
}

impl ViewKind {
    pub const ALL: &'static [ViewKind] = &[
        ViewKind::Patra,
        ViewKind::Sthana,
        ViewKind::Smriti,
        ViewKind::Navatama,
        ViewKind::Brihat,
        ViewKind::Antarvicaya,
        ViewKind::Sanketa,
        ViewKind::Itihasa,
        ViewKind::Parivartana,
        ViewKind::Sambandha,
        ViewKind::Ankita,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ViewKind::Patra => "Patra",
            ViewKind::Sthana => "Sthana",
            ViewKind::Smriti => "Smriti",
            ViewKind::Navatama => "Navatama",
            ViewKind::Brihat => "Brihat",
            ViewKind::Antarvicaya => "Antarvicaya",
            ViewKind::Sanketa => "Sanketa",
            ViewKind::Itihasa => "Itihasa",
            ViewKind::Parivartana => "Parivartana",
            ViewKind::Sambandha => "Sambandha",
            ViewKind::Ankita => "Ankita",
        }
    }

    pub fn english_hint(self) -> &'static str {
        match self {
            ViewKind::Patra => "Files",
            ViewKind::Sthana => "Directories",
            ViewKind::Smriti => "Recent",
            ViewKind::Navatama => "Modified",
            ViewKind::Brihat => "Large",
            ViewKind::Antarvicaya => "Content",
            ViewKind::Sanketa => "Symbols",
            ViewKind::Itihasa => "Git",
            ViewKind::Parivartana => "Changed",
            ViewKind::Sambandha => "Related",
            ViewKind::Ankita => "Pinned",
        }
    }

    pub fn is_enabled(self) -> bool {
        matches!(self, ViewKind::Patra | ViewKind::Sthana)
    }
}

/// State for the Drishti switcher overlay.
pub struct DrishtiSwitcherState {
    pub selected_index: usize,
}

impl DrishtiSwitcherState {
    pub fn new() -> Self {
        Self { selected_index: 0 }
    }

    pub fn selected_view(&self) -> ViewKind {
        ViewKind::ALL[self
            .selected_index
            .min(ViewKind::ALL.len().saturating_sub(1))]
    }

    pub fn select_next(&mut self) {
        self.selected_index = (self.selected_index + 1) % ViewKind::ALL.len();
    }

    pub fn select_previous(&mut self) {
        self.selected_index = if self.selected_index == 0 {
            ViewKind::ALL.len().saturating_sub(1)
        } else {
            self.selected_index - 1
        };
    }
}

impl Default for DrishtiSwitcherState {
    fn default() -> Self {
        Self::new()
    }
}

/// Preview state for the selected item.
pub struct PreviewState {
    pub is_visible: bool,
    pub is_loading: bool,
    pub truncated: bool,
    pub path: Option<String>,
    pub title: String,
    pub lines: Vec<String>,
    pub scroll: u16,
}

impl PreviewState {
    pub fn new() -> Self {
        Self {
            is_visible: true,
            is_loading: false,
            truncated: false,
            path: None,
            title: String::new(),
            lines: Vec::new(),
            scroll: 0,
        }
    }

    pub fn clear(&mut self) {
        self.is_loading = false;
        self.truncated = false;
        self.path = None;
        self.title.clear();
        self.lines.clear();
        self.scroll = 0;
    }

    pub fn toggle(&mut self) {
        self.is_visible = !self.is_visible;
    }
}

impl Default for PreviewState {
    fn default() -> Self {
        Self::new()
    }
}
