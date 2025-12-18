//! Main application loop and event handling.

use crate::state::{AppMode, AppState};
use crate::ui;
use crate::worker::{start_worker, WorkerCommand, WorkerEvent};
use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    Frame, Terminal,
};
use std::io;
use std::sync::mpsc;

/// Open a file in the user's preferred editor
fn open_file_in_editor(path: &str) -> Result<()> {
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

    // Execute editor and wait for it to complete
    Command::new(&editor)
        .arg(path)
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to open editor '{}': {}", editor, e))?;

    Ok(())
}

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

    let (cmd_tx, cmd_rx) = mpsc::channel::<WorkerCommand>();
    let (evt_tx, evt_rx) = mpsc::channel::<WorkerEvent>();
    let worker_handle = start_worker(cmd_rx, evt_tx);

    // Run the main loop
    let res = run_app(&mut terminal, &mut app, cmd_tx.clone(), evt_rx);

    let _ = cmd_tx.send(WorkerCommand::Quit);
    let _ = worker_handle.join();

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

    // Open file in editor if requested
    if let Some(path) = app.open_in_editor {
        open_file_in_editor(&path)?;
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
    cmd_tx: mpsc::Sender<WorkerCommand>,
    evt_rx: mpsc::Receiver<WorkerEvent>,
) -> Result<()> {
    let mut last_query = String::new();
    let mut last_search_sent_at = std::time::Instant::now();
    let mut last_view = app.view;
    let mut search_id: u64 = 0;
    let mut active_search_id: u64 = 0;

    let mut preview_id: u64 = 0;
    let mut active_preview_id: u64 = 0;
    let mut last_preview_path: Option<String> = None;

    let mut error_clear_time: Option<std::time::Instant> = None;

    loop {
        // Apply worker events
        while let Ok(evt) = evt_rx.try_recv() {
            match evt {
                WorkerEvent::Status { status } => {
                    app.daemon_status = status;
                }
                WorkerEvent::SearchResults { id, results, error } => {
                    if id == active_search_id {
                        app.search.set_results(results);
                        app.search.is_searching = false;
                        app.error = error;
                    }
                }
                WorkerEvent::PreviewReady {
                    id,
                    path,
                    title,
                    lines,
                    truncated,
                } => {
                    if id == active_preview_id {
                        app.preview.is_loading = false;
                        app.preview.truncated = truncated;
                        app.preview.path = Some(path);
                        app.preview.title = title;
                        app.preview.lines = lines;
                    }
                }
            }
        }

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

        // Re-run the current search when switching Drishti.
        if app.view != last_view {
            last_view = app.view;
            app.search.is_searching = true;
            search_id = search_id.wrapping_add(1);
            active_search_id = search_id;
            let _ = cmd_tx.send(WorkerCommand::Search {
                id: active_search_id,
                query: app.search.query.clone(),
                limit: 100,
                view: app.view,
            });
            last_search_sent_at = std::time::Instant::now();
        }

        // Check if query changed and trigger search (with debounce)
        if app.search.query != last_query {
            let elapsed = last_search_sent_at.elapsed();
            if elapsed > std::time::Duration::from_millis(150) || app.search.query.is_empty() {
                last_query = app.search.query.clone();
                app.search.is_searching = true;
                search_id = search_id.wrapping_add(1);
                active_search_id = search_id;
                let _ = cmd_tx.send(WorkerCommand::Search {
                    id: active_search_id,
                    query: app.search.query.clone(),
                    limit: 100,
                    view: app.view,
                });
                last_search_sent_at = std::time::Instant::now();
            }
        }

        // Schedule preview for selected result (best-effort).
        if app.preview.is_visible && app.mode == AppMode::Search {
            if let Some(result) = app.search.selected_result() {
                if last_preview_path.as_deref() != Some(result.path.as_str()) {
                    preview_id = preview_id.wrapping_add(1);
                    active_preview_id = preview_id;
                    last_preview_path = Some(result.path.clone());
                    app.preview.is_loading = true;
                    app.preview.truncated = false;
                    app.preview.path = Some(result.path.clone());
                    app.preview.title = result.name.clone();
                    app.preview.lines.clear();
                    app.preview.scroll = 0;
                    let _ = cmd_tx.send(WorkerCommand::Preview {
                        id: active_preview_id,
                        path: result.path.clone(),
                    });
                }
            } else if last_preview_path.is_some() {
                last_preview_path = None;
                app.preview.clear();
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
        AppMode::DrishtiSwitcher => handle_drishti_switcher_keys(app, key, modifiers),
        AppMode::Confirm(_) => handle_confirm_keys(app, key),
    }
}

/// Handle keys in Drishti switcher mode.
fn handle_drishti_switcher_keys(app: &mut AppState, key: KeyCode, modifiers: KeyModifiers) {
    match (key, modifiers) {
        (KeyCode::Esc, _) => app.toggle_drishti_switcher(),
        (KeyCode::Char('t'), KeyModifiers::CONTROL) => app.toggle_drishti_switcher(),
        (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, KeyModifiers::NONE) => {
            app.ui.drishti_switcher.select_next();
        }
        (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, KeyModifiers::NONE) => {
            app.ui.drishti_switcher.select_previous();
        }
        (KeyCode::Enter, KeyModifiers::NONE) => {
            let selected = app.ui.drishti_switcher.selected_view();
            if selected.is_enabled() {
                app.view = selected;
                app.toggle_drishti_switcher();
            } else {
                app.error = Some(format!(
                    "Drishti '{}' ({}) is coming soon",
                    selected.label(),
                    selected.english_hint()
                ));
                app.toggle_drishti_switcher();
            }
        }
        _ => {}
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
        // Drishti switcher
        (KeyCode::Char('t'), KeyModifiers::CONTROL) => {
            app.toggle_drishti_switcher();
            return;
        }
        // Toggle preview
        (KeyCode::Char('o'), KeyModifiers::CONTROL) => {
            app.preview.toggle();
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
        // Preview scrolling (when visible)
        (KeyCode::PageUp, KeyModifiers::NONE) | (KeyCode::Char('u'), KeyModifiers::CONTROL)
            if app.preview.is_visible =>
        {
            app.preview.scroll = app.preview.scroll.saturating_sub(10);
        }
        (KeyCode::PageDown, KeyModifiers::NONE) | (KeyCode::Char('d'), KeyModifiers::CONTROL)
            if app.preview.is_visible =>
        {
            let max_scroll = app.preview.lines.len().saturating_sub(1) as u16;
            app.preview.scroll = (app.preview.scroll + 10).min(max_scroll);
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
    // Store path to open after TUI exits
    app.open_in_editor = Some(path.to_string());
    app.quit();
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
fn ui_render(f: &mut Frame, app: &mut AppState) {
    match app.mode {
        AppMode::Search => render_search(f, app),
        AppMode::Help => ui::overlays::render_help(f),
        AppMode::DrishtiSwitcher => {
            render_search(f, app);
            ui::overlays::render_drishti_switcher(f, app);
        }
        AppMode::Confirm(_) => ui::overlays::render_confirm(f, app),
    }
}

/// Render search interface.
fn render_search(f: &mut Frame, app: &mut AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Length(3), // Search input
            Constraint::Min(0),    // Body
            Constraint::Length(1), // Status bar
        ])
        .split(f.area());

    ui::header::render(f, chunks[0], app);
    ui::search_input::render(f, chunks[1], app);

    if app.preview.is_visible {
        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(chunks[2]);
        ui::results::render(f, body[0], app);
        ui::preview::render(f, body[1], app);
    } else {
        ui::results::render(f, chunks[2], app);
    }

    ui::footer::render(f, chunks[3], app);
}
