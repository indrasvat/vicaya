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

    loop {
        // Draw UI
        terminal.draw(|f| ui_render(f, app))?;

        // Check if query changed and trigger search
        if app.search.query != last_query {
            last_query = app.search.query.clone();
            app.perform_search();
        }

        // Handle events
        if event::poll(std::time::Duration::from_millis(100))? {
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
    match (key, modifiers) {
        // Quit
        (KeyCode::Char('c'), KeyModifiers::CONTROL) | (KeyCode::Char('q'), KeyModifiers::NONE) => {
            app.quit();
        }
        // Help
        (KeyCode::Char('?'), KeyModifiers::NONE) => {
            app.toggle_help();
        }
        // Navigation
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
        // Clear search
        (KeyCode::Esc, KeyModifiers::NONE) => {
            app.search.clear_query();
        }
        // Input
        (KeyCode::Char(c), KeyModifiers::NONE) | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
            app.search.insert_char(c);
        }
        (KeyCode::Backspace, KeyModifiers::NONE) => {
            app.search.delete_char();
        }
        (KeyCode::Left, KeyModifiers::NONE) => {
            app.search.move_cursor_left();
        }
        (KeyCode::Right, KeyModifiers::NONE) => {
            app.search.move_cursor_right();
        }
        _ => {}
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
        Span::styled(" - Fast File Search", Style::default().fg(ui::TEXT_SECONDARY)),
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

    let input = Paragraph::new(Line::from(vec![
        Span::styled("🔍 ", Style::default().fg(ui::ACCENT)),
        Span::styled(query, Style::default().fg(ui::TEXT_PRIMARY)),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(ui::BORDER_FOCUS))
            .style(Style::default().bg(ui::BG_ELEVATED)),
    );

    f.render_widget(input, area);

    // Show cursor
    if cursor_pos < query.len() {
        f.set_cursor_position((area.x + 3 + cursor_pos as u16, area.y + 1));
    } else {
        f.set_cursor_position((area.x + 3 + query.len() as u16, area.y + 1));
    }
}

/// Render results list
fn render_results(f: &mut Frame, area: Rect, app: &AppState) {
    let results = &app.search.results;
    let selected = app.search.selected_index;

    let items: Vec<ListItem> = results
        .iter()
        .enumerate()
        .map(|(i, result)| {
            let marker = if i == selected { "▸" } else { " " };
            let score_color = ui::score_color(result.score);

            let line = Line::from(vec![
                Span::styled(marker, Style::default().fg(ui::PRIMARY)),
                Span::raw(" "),
                Span::styled(&result.name, Style::default().fg(ui::TEXT_PRIMARY)),
                Span::raw(" "),
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

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(ui::BORDER_DIM))
            .title(format!("RESULTS ({})", results.len())),
    );

    f.render_widget(list, area);
}

/// Render status bar
fn render_status_bar(f: &mut Frame, area: Rect, app: &AppState) {
    let mut spans = vec![
        Span::styled("j/k:", Style::default().fg(ui::PRIMARY)),
        Span::styled(" ↑↓  ", Style::default().fg(ui::TEXT_SECONDARY)),
        Span::styled("Esc:", Style::default().fg(ui::PRIMARY)),
        Span::styled(" clear  ", Style::default().fg(ui::TEXT_SECONDARY)),
        Span::styled("?:", Style::default().fg(ui::PRIMARY)),
        Span::styled(" help  ", Style::default().fg(ui::TEXT_SECONDARY)),
        Span::styled("q:", Style::default().fg(ui::PRIMARY)),
        Span::styled(" quit", Style::default().fg(ui::TEXT_SECONDARY)),
    ];

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
        "Navigation:",
        "  j / ↓         Move down",
        "  k / ↑         Move up",
        "  g             Jump to top",
        "  G             Jump to bottom",
        "",
        "Actions:",
        "  Enter         Open file",
        "  Esc           Clear search",
        "",
        "System:",
        "  ?             Toggle help",
        "  q / Ctrl-c    Quit",
        "",
        "Press ? or Esc to close this help",
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
