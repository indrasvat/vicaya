//! Main application loop and event handling.

use crate::state::{AppMode, AppState};
use crate::ui;
use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};
use std::io;

/// Run the TUI application
pub fn run() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = AppState::new();

    // Run the main loop
    let res = run_app(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    // Print path if requested (for terminal integration)
    if let Some(path) = app.print_on_exit {
        println!("{}", path);
    }

    if let Err(err) = res {
        eprintln!("Error: {:?}", err);
    }

    Ok(())
}

/// Main application loop
fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut AppState,
) -> Result<()> {
    let mut last_query = String::new();
    let mut last_search_time = std::time::Instant::now();
    let mut error_clear_time: Option<std::time::Instant> = None;

    loop {
        // Draw UI
        terminal.draw(|f| ui_render(f, app))?;

        // Clear temporary success messages after 2 seconds
        if let Some(clear_time) = error_clear_time {
            if clear_time.elapsed() > std::time::Duration::from_secs(2) {
                if let Some(ref error) = app.error {
                    if error.starts_with('✓') {
                        app.error = None;
                        error_clear_time = None;
                    }
                }
            }
        } else if let Some(ref error) = app.error {
            if error.starts_with('✓') {
                error_clear_time = Some(std::time::Instant::now());
            }
        }

        // Check if query changed and trigger search (with debounce)
        if app.search.query != last_query {
            let elapsed = last_search_time.elapsed();
            if elapsed > std::time::Duration::from_millis(150) || app.search.query.is_empty() {
                last_query = app.search.query.clone();
                app.perform_search();
                last_search_time = std::time::Instant::now();
            }
        }

        // Handle events
        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                handle_key_event(app, key.code, key.modifiers);
            }
        }

        // Check if should quit
        if app.should_quit() {
            break;
        }
    }

    Ok(())
}

/// Handle keyboard events
fn handle_key_event(app: &mut AppState, key: KeyCode, modifiers: KeyModifiers) {
    match app.mode {
        AppMode::Search => handle_search_keys(app, key, modifiers),
        AppMode::Help => handle_help_keys(app, key),
        AppMode::Confirm(_) => handle_confirm_keys(app, key),
    }
}

/// Handle keys in search mode
fn handle_search_keys(app: &mut AppState, key: KeyCode, modifiers: KeyModifiers) {
    // Global keys that work regardless of focus
    match (key, modifiers) {
        // Quit
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
            app.quit();
            return;
        }
        // Help
        (KeyCode::Char('?'), KeyModifiers::NONE) if !app.search.is_input_focused() => {
            app.toggle_help();
            return;
        }
        // Toggle focus with Tab
        (KeyCode::Tab, KeyModifiers::NONE) => {
            app.search.toggle_focus();
            return;
        }
        // Escape clears search or changes focus
        (KeyCode::Esc, KeyModifiers::NONE) => {
            if app.search.is_results_focused() {
                app.search.focus = crate::state::FocusTarget::Input;
            } else {
                app.search.clear_query();
            }
            return;
        }
        _ => {}
    }

    // Focus-specific keys
    if app.search.is_input_focused() {
        handle_input_keys(app, key, modifiers);
    } else {
        handle_results_keys(app, key, modifiers);
    }
}

/// Handle keys when input is focused
fn handle_input_keys(app: &mut AppState, key: KeyCode, modifiers: KeyModifiers) {
    match (key, modifiers) {
        // Typing characters
        (KeyCode::Char(c), KeyModifiers::NONE) | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
            app.search.insert_char(c);
        }
        // Backspace
        (KeyCode::Backspace, KeyModifiers::NONE) => {
            app.search.delete_char();
        }
        // Cursor movement
        (KeyCode::Left, KeyModifiers::NONE) => {
            app.search.move_cursor_left();
        }
        (KeyCode::Right, KeyModifiers::NONE) => {
            app.search.move_cursor_right();
        }
        // Down arrow switches to results if there are any
        (KeyCode::Down, KeyModifiers::NONE) => {
            if !app.search.results.is_empty() {
                app.search.focus = crate::state::FocusTarget::Results;
            }
        }
        _ => {}
    }
}

/// Handle keys when results are focused
fn handle_results_keys(app: &mut AppState, key: KeyCode, modifiers: KeyModifiers) {
    match (key, modifiers) {
        // Up arrow at top goes back to input
        (KeyCode::Up, KeyModifiers::NONE) if app.search.selected_index == 0 => {
            app.search.focus = crate::state::FocusTarget::Input;
        }
        // Navigation - vi keys and arrows
        (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, KeyModifiers::NONE) => {
            app.search.select_next();
        }
        (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, KeyModifiers::NONE) => {
            app.search.select_previous();
        }
        (KeyCode::Char('g'), KeyModifiers::NONE) => {
            app.search.select_first();
        }
        (KeyCode::Char('G'), KeyModifiers::SHIFT) => {
            app.search.select_last();
        }
        // File actions
        (KeyCode::Enter, KeyModifiers::NONE) | (KeyCode::Char('o'), KeyModifiers::NONE) => {
            if let Some(path) = app.search.selected_result().map(|r| r.path.clone()) {
                open_in_editor(&path, app);
            }
        }
        (KeyCode::Char('y'), KeyModifiers::NONE) => {
            if let Some(path) = app.search.selected_result().map(|r| r.path.clone()) {
                copy_to_clipboard(&path, app);
            }
        }
        (KeyCode::Char('p'), KeyModifiers::NONE) => {
            if let Some(result) = app.search.selected_result() {
                app.print_on_exit = Some(result.path.clone());
                app.quit();
            }
        }
        (KeyCode::Char('r'), KeyModifiers::NONE) => {
            if let Some(path) = app.search.selected_result().map(|r| r.path.clone()) {
                reveal_in_finder(&path, app);
            }
        }
        // Quit
        (KeyCode::Char('q'), KeyModifiers::NONE) => {
            app.quit();
        }
        _ => {}
    }
}

/// Open file in $EDITOR or fallback editor
fn open_in_editor(path: &str, app: &mut AppState) {
    use std::process::Command;

    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| {
            // Fallback editors
            if cfg!(target_os = "macos") {
                "open".to_string()
            } else {
                "vim".to_string()
            }
        });

    // Quit TUI before opening editor
    app.should_quit = true;

    // Clone for thread
    let editor = editor.clone();
    let path = path.to_string();

    // Spawn editor after TUI exits
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        let _ = Command::new(&editor).arg(&path).status();
    });
}

/// Copy path to clipboard
fn copy_to_clipboard(path: &str, app: &mut AppState) {
    use std::process::Command;

    let result = if cfg!(target_os = "macos") {
        Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(stdin) = child.stdin.as_mut() {
                    stdin.write_all(path.as_bytes())?;
                }
                child.wait()
            })
    } else {
        Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(stdin) = child.stdin.as_mut() {
                    stdin.write_all(path.as_bytes())?;
                }
                child.wait()
            })
    };

    match result {
        Ok(_) => {
            app.error = Some(format!("✓ Copied: {}", path));
        }
        Err(e) => {
            app.error = Some(format!("Failed to copy: {}", e));
        }
    }
}

/// Reveal file in file manager
fn reveal_in_finder(path: &str, app: &mut AppState) {
    use std::process::Command;

    let result = if cfg!(target_os = "macos") {
        Command::new("open").args(["-R", path]).spawn()
    } else {
        // On Linux, open the parent directory
        let parent = std::path::Path::new(path)
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or(path);
        Command::new("xdg-open").arg(parent).spawn()
    };

    match result {
        Ok(_) => {
            app.error = Some(format!("✓ Revealed: {}", path));
        }
        Err(e) => {
            app.error = Some(format!("Failed to reveal: {}", e));
        }
    }
}

/// Handle keys in help mode
fn handle_help_keys(app: &mut AppState, key: KeyCode) {
    match key {
        KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q') => {
            app.toggle_help();
        }
        _ => {}
    }
}

/// Handle keys in confirm mode
fn handle_confirm_keys(_app: &mut AppState, _key: KeyCode) {
    // TODO: Implement confirmation dialog handling
}

/// Render the UI
fn ui_render(f: &mut Frame, app: &AppState) {
    match app.mode {
        AppMode::Search => render_search(f, app),
        AppMode::Help => render_help(f),
        AppMode::Confirm(_) => render_confirm(f, app),
    }
}

/// Render search interface
fn render_search(f: &mut Frame, app: &AppState) {
    // Create layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Length(3), // Search input
            Constraint::Min(0),    // Results
            Constraint::Length(1), // Status bar
        ])
        .split(f.area());

    render_header(f, chunks[0], app);
    render_search_input(f, chunks[1], app);
    render_results(f, chunks[2], app);
    render_status_bar(f, chunks[3], app);
}

/// Render header
fn render_header(f: &mut Frame, area: Rect, app: &AppState) {
    let mut spans = vec![
        Span::styled(
            "vicaya",
            Style::default()
                .fg(ui::PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " - Fast File Search",
            Style::default().fg(ui::TEXT_SECONDARY),
        ),
    ];

    // Add daemon status
    if let Some(status) = &app.daemon_status {
        spans.push(Span::styled("  ", Style::default()));
        spans.push(Span::styled(
            format!("📁 {} files", status.indexed_files),
            Style::default().fg(ui::INFO),
        ));
    } else {
        spans.push(Span::styled(
            "  ⚠️  daemon offline",
            Style::default().fg(ui::WARNING),
        ));
    }

    let header = Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(ui::BORDER_DIM)),
    );

    f.render_widget(header, area);
}

/// Render search input
fn render_search_input(f: &mut Frame, area: Rect, app: &AppState) {
    let query = &app.search.query;
    let cursor_pos = app.search.cursor_position;
    let is_focused = app.search.is_input_focused();

    // Use different border style based on focus
    let border_style = if is_focused {
        Style::default().fg(ui::BORDER_FOCUS)
    } else {
        Style::default().fg(ui::BORDER_DIM)
    };

    let input = Paragraph::new(Line::from(vec![
        Span::styled("> ", Style::default().fg(ui::ACCENT)),
        Span::styled(query, Style::default().fg(ui::TEXT_PRIMARY)),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .style(if is_focused {
                Style::default().bg(ui::BG_ELEVATED)
            } else {
                Style::default()
            }),
    );

    f.render_widget(input, area);

    // Show cursor only when input is focused
    if is_focused {
        // Cursor position: 1 (border) + 1 (space after >) + cursor_pos
        let cursor_x = area.x + 3 + cursor_pos as u16;
        let cursor_y = area.y + 1;
        f.set_cursor_position((cursor_x, cursor_y));
    }
}

/// Truncate path intelligently for display
fn truncate_path(path: &str, max_len: usize, show_full: bool) -> String {
    if show_full || path.len() <= max_len {
        return path.to_string();
    }

    // Show beginning and end with ... in the middle
    let start_len = max_len / 2;
    let end_len = max_len - start_len - 3; // Reserve 3 chars for "..."

    if path.len() > max_len {
        format!(
            "{}...{}",
            &path[..start_len],
            &path[path.len().saturating_sub(end_len)..]
        )
    } else {
        path.to_string()
    }
}

/// Render results list
fn render_results(f: &mut Frame, area: Rect, app: &AppState) {
    let results = &app.search.results;
    let selected = app.search.selected_index;

    // Calculate available width for path display (rough estimate)
    let available_width = area.width.saturating_sub(4); // Account for borders
    let max_path_len = available_width.saturating_sub(30) as usize; // Reserve space for name, score, marker

    let items: Vec<ListItem> = results
        .iter()
        .enumerate()
        .map(|(i, result)| {
            let marker = if i == selected { "▸" } else { " " };
            let score_color = ui::score_color(result.score);
            let is_selected = i == selected;

            // Get parent directory path (remove filename)
            let path = std::path::Path::new(&result.path);
            let dir_path = path
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or("");

            // Truncate path if not selected
            let display_path = truncate_path(dir_path, max_path_len.max(30), is_selected);

            let line = Line::from(vec![
                Span::styled(marker, Style::default().fg(ui::PRIMARY)),
                Span::raw(" "),
                Span::styled(&result.name, Style::default().fg(ui::TEXT_PRIMARY)),
                Span::raw(" "),
                Span::styled(
                    format!("({}) ", display_path),
                    Style::default().fg(ui::TEXT_MUTED).add_modifier(Modifier::DIM),
                ),
                Span::styled(
                    format!("{:.2}", result.score),
                    Style::default().fg(score_color),
                ),
            ]);

            let style = if i == selected {
                Style::default().bg(ui::BG_ELEVATED)
            } else {
                Style::default()
            };

            ListItem::new(line).style(style)
        })
        .collect();

    // Show focused border when results are focused
    let border_style = if app.search.is_results_focused() {
        Style::default().fg(ui::BORDER_FOCUS)
    } else {
        Style::default().fg(ui::BORDER_DIM)
    };

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(format!("RESULTS ({})", results.len())),
    );

    f.render_widget(list, area);
}

/// Render status bar
fn render_status_bar(f: &mut Frame, area: Rect, app: &AppState) {
    let mut spans = vec![
        Span::styled("Tab:", Style::default().fg(ui::PRIMARY)),
        Span::styled(" switch  ", Style::default().fg(ui::TEXT_SECONDARY)),
    ];

    // Add focus-specific hints
    if app.search.is_results_focused() {
        spans.extend(vec![
            Span::styled("↵:", Style::default().fg(ui::PRIMARY)),
            Span::styled(" open  ", Style::default().fg(ui::TEXT_SECONDARY)),
            Span::styled("y:", Style::default().fg(ui::PRIMARY)),
            Span::styled(" copy  ", Style::default().fg(ui::TEXT_SECONDARY)),
            Span::styled("p:", Style::default().fg(ui::PRIMARY)),
            Span::styled(" print  ", Style::default().fg(ui::TEXT_SECONDARY)),
            Span::styled("r:", Style::default().fg(ui::PRIMARY)),
            Span::styled(" reveal  ", Style::default().fg(ui::TEXT_SECONDARY)),
        ]);
    }

    spans.extend(vec![
        Span::styled("Esc:", Style::default().fg(ui::PRIMARY)),
        Span::styled(" clear  ", Style::default().fg(ui::TEXT_SECONDARY)),
        Span::styled("Ctrl-c:", Style::default().fg(ui::PRIMARY)),
        Span::styled(" quit", Style::default().fg(ui::TEXT_SECONDARY)),
    ]);

    // Show error if any
    if let Some(error) = &app.error {
        spans.push(Span::styled("  ", Style::default()));
        spans.push(Span::styled(
            format!("⚠️  {}", error),
            Style::default().fg(ui::ERROR),
        ));
    }

    let hints = Line::from(spans);
    let status = Paragraph::new(hints).style(Style::default().bg(ui::BG_SURFACE));

    f.render_widget(status, area);
}

/// Render help overlay
fn render_help(f: &mut Frame) {
    let help_text = vec![
        "Vicaya - Fast File Search TUI",
        "",
        "Focus:",
        "  Tab           Switch between input/results",
        "  ↓ (in input)  Move to results",
        "  ↑ (at top)    Move to input",
        "",
        "Navigation (when results focused):",
        "  j / ↓         Move down",
        "  k / ↑         Move up",
        "  g             Jump to top",
        "  G             Jump to bottom",
        "",
        "File Actions (when results focused):",
        "  Enter / o     Open in $EDITOR",
        "  y             Copy path to clipboard",
        "  p             Print path and exit (terminal)",
        "  r             Reveal in file manager",
        "",
        "Other:",
        "  Esc           Clear search / back to input",
        "  Ctrl-c        Quit",
        "",
        "Press Esc to close this help",
    ];

    let help = Paragraph::new(help_text.join("\n"))
        .style(Style::default().fg(ui::TEXT_PRIMARY).bg(ui::BG_DARK))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(ui::PRIMARY))
                .title(" Help "),
        );

    let area = centered_rect(60, 70, f.area());
    f.render_widget(help, area);
}

/// Render confirmation dialog
fn render_confirm(f: &mut Frame, _app: &AppState) {
    let confirm = Paragraph::new("Are you sure? (y/n)")
        .style(Style::default().fg(ui::TEXT_PRIMARY).bg(ui::BG_DARK))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(ui::WARNING)),
        );

    let area = centered_rect(40, 20, f.area());
    f.render_widget(confirm, area);
}

/// Helper to create centered rect
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
