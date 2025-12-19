//! Application state management.

use crate::client::DaemonStatus;
use std::path::{Path, PathBuf};
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
    /// Search within preview
    PreviewSearch,
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
    /// Current Ksetra (scope) stack
    pub ksetra: KsetraState,
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
            ksetra: KsetraState::new(),
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
            _ => {
                self.ui.drishti_switcher.reset();
                AppMode::DrishtiSwitcher
            }
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
    /// Preview pane is focused
    Preview,
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
            FocusTarget::Preview => FocusTarget::Input,
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

    /// Check if preview is focused
    pub fn is_preview_focused(&self) -> bool {
        self.focus == FocusTarget::Preview
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
    /// Viewport height for preview content (in lines, excluding borders)
    pub preview_viewport_height: usize,
    /// Varga (grouping) mode
    pub grouping: GroupingMode,
    /// Drishti switcher state
    pub drishti_switcher: DrishtiSwitcherState,
}

impl UiState {
    /// Create a new UI state
    pub fn new() -> Self {
        Self {
            scroll_offset: 0,
            viewport_height: 0,
            preview_viewport_height: 0,
            grouping: GroupingMode::None,
            drishti_switcher: DrishtiSwitcherState::new(),
        }
    }

    /// Update scroll offset to keep selection visible
    pub fn update_scroll(&mut self, selected_row: usize, total_rows: usize) {
        let total_rows = total_rows.max(1);
        let selected_row = selected_row.min(total_rows.saturating_sub(1));

        // Ensure selected item is visible
        if selected_row < self.scroll_offset {
            self.scroll_offset = selected_row;
        } else if selected_row >= self.scroll_offset + self.viewport_height {
            self.scroll_offset = selected_row.saturating_sub(self.viewport_height - 1);
        }

        self.scroll_offset = self.scroll_offset.min(total_rows.saturating_sub(1));
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

/// Varga (grouping) mode for the results list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupingMode {
    None,
    Directory,
    Extension,
}

impl GroupingMode {
    pub fn label(self) -> &'static str {
        match self {
            GroupingMode::None => "none",
            GroupingMode::Directory => "dir",
            GroupingMode::Extension => "ext",
        }
    }

    pub fn next(self) -> Self {
        match self {
            GroupingMode::None => GroupingMode::Directory,
            GroupingMode::Directory => GroupingMode::Extension,
            GroupingMode::Extension => GroupingMode::None,
        }
    }
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

/// Ksetra (scope) state with stack-based navigation.
#[derive(Debug, Clone, Default)]
pub struct KsetraState {
    stack: Vec<PathBuf>,
}

impl KsetraState {
    pub fn new() -> Self {
        Self { stack: Vec::new() }
    }

    pub fn current(&self) -> Option<&PathBuf> {
        self.stack.last()
    }

    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    pub fn is_global(&self) -> bool {
        self.stack.is_empty()
    }

    pub fn push(&mut self, path: PathBuf) {
        self.stack.push(path);
    }

    pub fn pop(&mut self) -> Option<PathBuf> {
        self.stack.pop()
    }

    pub fn breadcrumbs(&self) -> String {
        if self.stack.is_empty() {
            return "global".to_string();
        }

        let mut parts: Vec<String> = Vec::with_capacity(self.stack.len());
        for (idx, p) in self.stack.iter().enumerate() {
            if idx == 0 {
                parts.push(pretty_path(p));
                continue;
            }

            if let Some(prev) = self.stack.get(idx.saturating_sub(1)) {
                if p.starts_with(prev) {
                    if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                        parts.push(name.to_string());
                        continue;
                    }
                }
            }

            parts.push(pretty_path(p));
        }

        parts.join(" ▸ ")
    }
}

fn pretty_path(path: &Path) -> String {
    let Ok(home) = std::env::var("HOME") else {
        return path.display().to_string();
    };

    let home = PathBuf::from(home);
    if path.starts_with(&home) {
        if let Ok(rest) = path.strip_prefix(&home) {
            let rest = rest.display().to_string();
            if rest.is_empty() {
                return "~".to_string();
            }
            return format!("~/{rest}");
        }
    }

    path.display().to_string()
}

/// Parsed query into a daemon search term + Niyama filters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedQuery {
    pub term: String,
    pub niyamas: Vec<Niyama>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NiyamaType {
    File,
    Dir,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Lt,
    Lte,
    Gt,
    Gte,
    Eq,
}

impl CmpOp {
    pub fn matches_i64(self, left: i64, right: i64) -> bool {
        match self {
            CmpOp::Lt => left < right,
            CmpOp::Lte => left <= right,
            CmpOp::Gt => left > right,
            CmpOp::Gte => left >= right,
            CmpOp::Eq => left == right,
        }
    }

    pub fn matches_u64(self, left: u64, right: u64) -> bool {
        match self {
            CmpOp::Lt => left < right,
            CmpOp::Lte => left <= right,
            CmpOp::Gt => left > right,
            CmpOp::Gte => left >= right,
            CmpOp::Eq => left == right,
        }
    }

    pub fn invert(self) -> Self {
        match self {
            CmpOp::Lt => CmpOp::Gt,
            CmpOp::Lte => CmpOp::Gte,
            CmpOp::Gt => CmpOp::Lt,
            CmpOp::Gte => CmpOp::Lte,
            CmpOp::Eq => CmpOp::Eq,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CmpI64 {
    pub op: CmpOp,
    pub value: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CmpU64 {
    pub op: CmpOp,
    pub value: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Niyama {
    Type { kind: NiyamaType, raw: String },
    Ext { exts: Vec<String>, raw: String },
    Path { needle: String, raw: String },
    Mtime { cmp: CmpI64, raw: String },
    Size { cmp: CmpU64, raw: String },
}

impl Niyama {
    pub fn raw(&self) -> &str {
        match self {
            Niyama::Type { raw, .. }
            | Niyama::Ext { raw, .. }
            | Niyama::Path { raw, .. }
            | Niyama::Mtime { raw, .. }
            | Niyama::Size { raw, .. } => raw,
        }
    }
}

pub fn parse_query(raw: &str) -> ParsedQuery {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let mut term_tokens: Vec<&str> = Vec::new();

    let mut type_filter: Option<NiyamaType> = None;
    let mut exts: Vec<String> = Vec::new();
    let mut path_filters: Vec<Niyama> = Vec::new();
    let mut mtime: Option<CmpI64> = None;
    let mut mtime_raw: Option<String> = None;
    let mut size: Option<CmpU64> = None;
    let mut size_raw: Option<String> = None;

    for token in raw.split_whitespace() {
        if let Some(value) = token.strip_prefix("type:") {
            if let Some(kind) = parse_type(value) {
                type_filter = Some(kind);
                continue;
            }
        }

        if let Some(value) = token.strip_prefix("ext:") {
            let mut parsed = parse_exts(value);
            if !parsed.is_empty() {
                exts.append(&mut parsed);
                continue;
            }
        }

        if let Some(value) = token.strip_prefix("path:") {
            if !value.is_empty() {
                path_filters.push(Niyama::Path {
                    needle: value.to_lowercase(),
                    raw: token.to_string(),
                });
                continue;
            }
        }

        if let Some(value) = token.strip_prefix("mtime:") {
            if let Some(cmp) = parse_mtime_expr(value, now) {
                mtime = Some(cmp);
                mtime_raw = Some(token.to_string());
                continue;
            }
        }

        if let Some(value) = token.strip_prefix("size:") {
            if let Some(cmp) = parse_size_expr(value) {
                size = Some(cmp);
                size_raw = Some(token.to_string());
                continue;
            }
        }

        term_tokens.push(token);
    }

    exts.sort();
    exts.dedup();

    let mut niyamas: Vec<Niyama> = Vec::new();
    if let Some(kind) = type_filter {
        niyamas.push(Niyama::Type {
            kind,
            raw: format!(
                "type:{}",
                match kind {
                    NiyamaType::File => "file",
                    NiyamaType::Dir => "dir",
                }
            ),
        });
    }

    if !exts.is_empty() {
        niyamas.push(Niyama::Ext {
            raw: format!("ext:{}", exts.join(",")),
            exts,
        });
    }

    niyamas.extend(path_filters);

    if let (Some(cmp), Some(raw)) = (mtime, mtime_raw) {
        niyamas.push(Niyama::Mtime { cmp, raw });
    }

    if let (Some(cmp), Some(raw)) = (size, size_raw) {
        niyamas.push(Niyama::Size { cmp, raw });
    }

    ParsedQuery {
        term: term_tokens.join(" "),
        niyamas,
    }
}

fn parse_type(value: &str) -> Option<NiyamaType> {
    match value.trim().to_lowercase().as_str() {
        "file" | "f" => Some(NiyamaType::File),
        "dir" | "d" | "directory" => Some(NiyamaType::Dir),
        _ => None,
    }
}

fn parse_exts(value: &str) -> Vec<String> {
    value
        .split(',')
        .filter_map(|ext| {
            let ext = ext.trim().trim_start_matches('.').to_lowercase();
            if ext.is_empty() {
                None
            } else {
                Some(ext)
            }
        })
        .collect()
}

fn parse_op_and_value(input: &str) -> Option<(CmpOp, &str)> {
    let s = input.trim();
    if let Some(rest) = s.strip_prefix(">=") {
        return Some((CmpOp::Gte, rest));
    }
    if let Some(rest) = s.strip_prefix("<=") {
        return Some((CmpOp::Lte, rest));
    }
    if let Some(rest) = s.strip_prefix('>') {
        return Some((CmpOp::Gt, rest));
    }
    if let Some(rest) = s.strip_prefix('<') {
        return Some((CmpOp::Lt, rest));
    }
    if let Some(rest) = s.strip_prefix('=') {
        return Some((CmpOp::Eq, rest));
    }
    None
}

fn parse_size_expr(input: &str) -> Option<CmpU64> {
    let (op, value) = parse_op_and_value(input)?;
    let value = value.trim().to_lowercase();
    let (num_str, unit) = value
        .trim()
        .chars()
        .position(|c| !c.is_ascii_digit())
        .map(|idx| (&value[..idx], &value[idx..]))
        .unwrap_or((value.as_str(), ""));

    let n: u64 = num_str.parse().ok()?;
    let multiplier: u64 = match unit {
        "" | "b" => 1,
        "k" | "kb" => 1024,
        "m" | "mb" => 1024 * 1024,
        "g" | "gb" => 1024 * 1024 * 1024,
        "t" | "tb" => 1024 * 1024 * 1024 * 1024,
        _ => return None,
    };

    Some(CmpU64 {
        op,
        value: n.saturating_mul(multiplier),
    })
}

fn parse_mtime_expr(input: &str, now: i64) -> Option<CmpI64> {
    use chrono::{Local, NaiveDate, TimeZone};

    let (op, value) = parse_op_and_value(input)?;
    let value = value.trim();

    if let Some((n, unit)) = parse_duration(value) {
        let seconds = match unit {
            's' => n,
            'm' => n * 60,
            'h' => n * 60 * 60,
            'd' => n * 60 * 60 * 24,
            'w' => n * 60 * 60 * 24 * 7,
            _ => return None,
        };

        let threshold = now.saturating_sub(seconds);
        return Some(CmpI64 {
            op: op.invert(),
            value: threshold,
        });
    }

    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        let naive = date.and_hms_opt(0, 0, 0)?;
        let timestamp = match Local.from_local_datetime(&naive) {
            chrono::LocalResult::Single(dt) => dt.timestamp(),
            chrono::LocalResult::Ambiguous(dt, _) => dt.timestamp(),
            chrono::LocalResult::None => {
                chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc)
                    .timestamp()
            }
        };

        return Some(CmpI64 {
            op,
            value: timestamp,
        });
    }

    None
}

fn parse_duration(input: &str) -> Option<(i64, char)> {
    let s = input.trim().to_lowercase();
    let mut chars = s.chars();
    let unit = chars.next_back()?;
    let number = chars.as_str().parse::<i64>().ok()?;
    Some((number, unit))
}

/// State for the Drishti switcher overlay.
pub struct DrishtiSwitcherState {
    pub selected_index: usize,
    pub filter: String,
}

impl DrishtiSwitcherState {
    pub fn new() -> Self {
        Self {
            selected_index: 0,
            filter: String::new(),
        }
    }

    pub fn reset(&mut self) {
        self.selected_index = 0;
        self.filter.clear();
    }

    pub fn filter_query(&self) -> &str {
        self.filter.as_str()
    }

    pub fn push_filter_char(&mut self, c: char) {
        self.filter.push(c);
        self.selected_index = 0;
    }

    pub fn pop_filter_char(&mut self) {
        let _ = self.filter.pop();
        self.selected_index = 0;
    }

    pub fn matching_views(&self) -> Vec<ViewKind> {
        let needle = self.filter.trim().to_lowercase();
        if needle.is_empty() {
            return ViewKind::ALL.to_vec();
        }

        ViewKind::ALL
            .iter()
            .copied()
            .filter(|view| {
                view.label().to_lowercase().contains(&needle)
                    || view.english_hint().to_lowercase().contains(&needle)
            })
            .collect()
    }

    pub fn selected_view(&self) -> Option<ViewKind> {
        let views = self.matching_views();
        views.get(self.selected_index).copied()
    }

    pub fn select_next(&mut self) {
        let views = self.matching_views();
        if views.is_empty() {
            self.selected_index = 0;
            return;
        }
        self.selected_index = (self.selected_index + 1) % views.len();
    }

    pub fn select_previous(&mut self) {
        let views = self.matching_views();
        if views.is_empty() {
            self.selected_index = 0;
            return;
        }
        self.selected_index = if self.selected_index == 0 {
            views.len().saturating_sub(1)
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextKind {
    #[default]
    Normal,
    Meta,
    Error,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TextStyle {
    pub kind: TextKind,
    pub fg: Option<(u8, u8, u8)>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

#[derive(Debug, Clone)]
pub struct StyledSegment {
    pub text: String,
    pub style: TextStyle,
}

pub type StyledLine = Vec<StyledSegment>;

pub fn compute_content_line_numbers(lines: &[StyledLine]) -> Vec<Option<usize>> {
    let mut next = 0usize;
    lines
        .iter()
        .map(|line| {
            let is_meta =
                !line.is_empty() && line.iter().all(|seg| seg.style.kind == TextKind::Meta);
            let is_error = line.iter().any(|seg| seg.style.kind == TextKind::Error);

            if is_meta || is_error {
                None
            } else {
                next += 1;
                Some(next)
            }
        })
        .collect()
}

/// Preview state for the selected item.
pub struct PreviewState {
    pub is_visible: bool,
    pub is_loading: bool,
    pub truncated: bool,
    pub path: Option<String>,
    pub title: String,
    pub lines: Vec<StyledLine>,
    pub content_line_numbers: Vec<Option<usize>>,
    pub scroll: u16,
    pub show_line_numbers: bool,
    pub search_query: String,
    pub search_input: String,
    pub search_cursor: usize,
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
            content_line_numbers: Vec::new(),
            scroll: 0,
            show_line_numbers: false,
            search_query: String::new(),
            search_input: String::new(),
            search_cursor: 0,
        }
    }

    pub fn clear(&mut self) {
        self.is_loading = false;
        self.truncated = false;
        self.path = None;
        self.title.clear();
        self.lines.clear();
        self.content_line_numbers.clear();
        self.scroll = 0;
    }

    pub fn toggle(&mut self) {
        self.is_visible = !self.is_visible;
    }

    pub fn toggle_line_numbers(&mut self) {
        self.show_line_numbers = !self.show_line_numbers;
    }

    pub fn start_search(&mut self) {
        self.search_input = self.search_query.clone();
        self.search_cursor = self.search_input.len();
    }

    pub fn cancel_search(&mut self) {
        self.search_input = self.search_query.clone();
        self.search_cursor = self.search_input.len();
    }

    pub fn apply_search(&mut self) {
        self.search_query = self.search_input.trim().to_string();
        self.search_cursor = self.search_input.len();
    }

    pub fn clear_search(&mut self) {
        self.search_query.clear();
        self.search_input.clear();
        self.search_cursor = 0;
    }

    pub fn insert_search_char(&mut self, c: char) {
        self.search_input.insert(self.search_cursor, c);
        self.search_cursor += 1;
    }

    pub fn delete_search_char(&mut self) {
        if self.search_cursor == 0 {
            return;
        }
        self.search_input.remove(self.search_cursor - 1);
        self.search_cursor -= 1;
    }

    pub fn move_search_cursor_left(&mut self) {
        if self.search_cursor > 0 {
            self.search_cursor -= 1;
        }
    }

    pub fn move_search_cursor_right(&mut self) {
        if self.search_cursor < self.search_input.len() {
            self.search_cursor += 1;
        }
    }
}

impl Default for PreviewState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_query_extracts_term_and_filters() {
        let parsed = parse_query("foo ext:rs,md type:file path:src/");
        assert_eq!(parsed.term, "foo");
        assert_eq!(parsed.niyamas.len(), 3);
        assert!(matches!(
            parsed.niyamas[0],
            Niyama::Type {
                kind: NiyamaType::File,
                ..
            }
        ));
        assert!(matches!(parsed.niyamas[1], Niyama::Ext { .. }));
        assert!(matches!(parsed.niyamas[2], Niyama::Path { .. }));
    }

    #[test]
    fn parse_size_expr_parses_units() {
        let cmp = parse_size_expr(">10mb").unwrap();
        assert_eq!(cmp.op, CmpOp::Gt);
        assert_eq!(cmp.value, 10 * 1024 * 1024);
    }

    #[test]
    fn parse_mtime_expr_inverts_relative_age() {
        let cmp = parse_mtime_expr("<7d", 1000).unwrap();
        assert_eq!(cmp.op, CmpOp::Gt);
        assert_eq!(cmp.value, 1000 - 7 * 60 * 60 * 24);
    }

    #[test]
    fn ksetra_breadcrumbs_show_stack() {
        let mut ksetra = KsetraState::new();
        assert_eq!(ksetra.breadcrumbs(), "global");
        ksetra.push(PathBuf::from("/tmp/project"));
        ksetra.push(PathBuf::from("/tmp/project/src"));
        assert_eq!(ksetra.breadcrumbs(), "/tmp/project ▸ src");
    }

    #[test]
    fn compute_content_line_numbers_skips_meta() {
        let lines = vec![
            vec![StyledSegment {
                text: "meta".to_string(),
                style: TextStyle {
                    kind: TextKind::Meta,
                    ..Default::default()
                },
            }],
            vec![StyledSegment {
                text: "".to_string(),
                style: TextStyle {
                    kind: TextKind::Meta,
                    ..Default::default()
                },
            }],
            vec![StyledSegment {
                text: "a".to_string(),
                style: TextStyle::default(),
            }],
            vec![StyledSegment {
                text: "b".to_string(),
                style: TextStyle::default(),
            }],
        ];

        let nums = compute_content_line_numbers(&lines);
        assert_eq!(nums, vec![None, None, Some(1), Some(2)]);
    }
}
