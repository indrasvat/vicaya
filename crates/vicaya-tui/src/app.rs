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
use vicaya_core::smriti::SmritiAction;

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
pub fn run(startup_scope: Option<std::path::PathBuf>) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = AppState::with_startup_scope(startup_scope);

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
    let mut last_ksetra = app.ksetra.current().cloned();
    let mut search_id: u64 = 0;
    let mut active_search_id: u64 = 0;

    let mut preview_id: u64 = 0;
    let mut active_preview_id: u64 = 0;
    let mut last_preview_path: Option<String> = None;

    let mut error_clear_time: Option<std::time::Instant> = None;

    // Trigger initial search to populate recent files on startup
    trigger_search(
        &cmd_tx,
        app,
        &mut search_id,
        &mut active_search_id,
        &mut last_search_sent_at,
    );

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
                    anchor_line,
                } => {
                    if id == active_preview_id {
                        app.preview.is_loading = false;
                        app.preview.truncated = truncated;
                        app.preview.path = Some(path);
                        app.preview.title = title;
                        app.preview.lines = lines;
                        app.preview.content_line_numbers =
                            crate::state::compute_content_line_numbers(&app.preview.lines);
                        if let Some(line) = anchor_line {
                            app.preview.scroll = preview_scroll_for_line(app, line);
                        }
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

        // Re-run the current search when switching drishti.
        if app.view != last_view {
            last_view = app.view;
            trigger_search(
                &cmd_tx,
                app,
                &mut search_id,
                &mut active_search_id,
                &mut last_search_sent_at,
            );
        }

        // Re-run the current search when changing ksetra scope.
        if app.ksetra.current() != last_ksetra.as_ref() {
            last_ksetra = app.ksetra.current().cloned();
            trigger_search(
                &cmd_tx,
                app,
                &mut search_id,
                &mut active_search_id,
                &mut last_search_sent_at,
            );
        }

        // Check if query changed and trigger search (with debounce)
        if app.search.query != last_query {
            let elapsed = last_search_sent_at.elapsed();
            if elapsed > std::time::Duration::from_millis(150) || app.search.query.is_empty() {
                last_query = app.search.query.clone();
                trigger_search(
                    &cmd_tx,
                    app,
                    &mut search_id,
                    &mut active_search_id,
                    &mut last_search_sent_at,
                );
            }
        }

        // Schedule preview for selected result (best-effort).
        if app.preview.is_visible && app.mode == AppMode::Search {
            if let Some(result) = app.search.selected_result() {
                let anchor_line = content_result_anchor(app.view, result);
                let preview_key = anchor_line
                    .map(|line| format!("{}#{line}", result.path))
                    .unwrap_or_else(|| result.path.clone());
                if last_preview_path.as_deref() != Some(preview_key.as_str()) {
                    preview_id = preview_id.wrapping_add(1);
                    active_preview_id = preview_id;
                    last_preview_path = Some(preview_key);
                    app.preview.is_loading = true;
                    app.preview.truncated = false;
                    app.preview.path = Some(result.path.clone());
                    app.preview.title = result.name.clone();
                    app.preview.lines.clear();
                    app.preview.content_line_numbers.clear();
                    app.preview.scroll = 0;
                    if anchor_line.is_some() {
                        app.preview.search_query =
                            crate::state::parse_query(&app.search.query).term;
                    } else if app.view != crate::state::ViewKind::Antarvicaya {
                        app.preview.clear_search();
                    }
                    let _ = cmd_tx.send(WorkerCommand::Preview {
                        id: active_preview_id,
                        path: result.path.clone(),
                        anchor_line,
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
                for event in app.smriti_events.drain(..) {
                    let _ = cmd_tx.send(WorkerCommand::RecordSmriti {
                        path: event.path,
                        query: event.query,
                        action: event.action,
                    });
                }
                for path in app.smriti_forget_paths.drain(..) {
                    let _ = cmd_tx.send(WorkerCommand::ForgetSmriti { path });
                }
            }
        }

        // Check if should quit
        if app.should_quit() {
            break;
        }
    }

    Ok(())
}

fn content_result_anchor(
    view: crate::state::ViewKind,
    result: &vicaya_index::SearchResult,
) -> Option<usize> {
    if view != crate::state::ViewKind::Antarvicaya {
        return None;
    }

    let file_name = std::path::Path::new(&result.path)
        .file_name()
        .and_then(|name| name.to_str())?;
    let rest = result.name.strip_prefix(file_name)?.strip_prefix(':')?;
    rest.split(':').next()?.parse().ok()
}

fn preview_scroll_for_line(app: &AppState, line: usize) -> u16 {
    let target = app
        .preview
        .content_line_numbers
        .iter()
        .position(|number| *number == Some(line))
        .unwrap_or(0);
    target.saturating_sub(3) as u16
}

/// Handle keyboard events
fn handle_key_event(app: &mut AppState, key: KeyCode, modifiers: KeyModifiers) {
    match app.mode {
        AppMode::Search => handle_search_keys(app, key, modifiers),
        AppMode::Help => handle_help_keys(app, key),
        AppMode::DrishtiSwitcher => handle_drishti_switcher_keys(app, key, modifiers),
        AppMode::KriyaSuchi => handle_kriya_suchi_keys(app, key, modifiers),
        AppMode::PreviewSearch => handle_preview_search_keys(app, key, modifiers),
        AppMode::KsetraInput => handle_ksetra_input_keys(app, key, modifiers),
        AppMode::Confirm(_) => handle_confirm_keys(app, key),
    }
}

/// Handle keys in drishti switcher mode.
fn handle_drishti_switcher_keys(app: &mut AppState, key: KeyCode, modifiers: KeyModifiers) {
    match (key, modifiers) {
        (KeyCode::Esc, _) => app.toggle_drishti_switcher(),
        (KeyCode::Char('t'), KeyModifiers::CONTROL) => app.toggle_drishti_switcher(),
        (KeyCode::Backspace, KeyModifiers::NONE) => {
            app.ui.drishti_switcher.pop_filter_char();
        }
        (KeyCode::Down, KeyModifiers::NONE) => {
            app.ui.drishti_switcher.select_next();
        }
        (KeyCode::Up, KeyModifiers::NONE) => {
            app.ui.drishti_switcher.select_previous();
        }
        (KeyCode::Char('j'), KeyModifiers::NONE) => {
            if app.ui.drishti_switcher.filter_query().is_empty() {
                app.ui.drishti_switcher.select_next();
            } else {
                app.ui.drishti_switcher.push_filter_char('j');
            }
        }
        (KeyCode::Char('k'), KeyModifiers::NONE) => {
            if app.ui.drishti_switcher.filter_query().is_empty() {
                app.ui.drishti_switcher.select_previous();
            } else {
                app.ui.drishti_switcher.push_filter_char('k');
            }
        }
        (KeyCode::Enter, KeyModifiers::NONE) => {
            let Some(selected) = app.ui.drishti_switcher.selected_view() else {
                return;
            };
            if selected.is_enabled() {
                app.view = selected;
                app.toggle_drishti_switcher();
            } else {
                app.error = Some(format!(
                    "drishti '{}' ({}) is coming soon",
                    selected.label(),
                    selected.english_hint()
                ));
                app.toggle_drishti_switcher();
            }
        }
        (KeyCode::Char(c), KeyModifiers::NONE) => {
            if !c.is_whitespace() {
                app.ui.drishti_switcher.push_filter_char(c);
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
        // Kriya-Suchi (action palette)
        (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
            app.toggle_kriya_suchi();
            return;
        }
        // Toggle preview
        (KeyCode::Char('o'), KeyModifiers::CONTROL) => {
            app.preview.toggle();
            if !app.preview.is_visible && app.search.is_preview_focused() {
                app.search.focus = crate::state::FocusTarget::Results;
            }
            return;
        }
        // Cycle Varga (grouping)
        (KeyCode::Char('g'), KeyModifiers::CONTROL) => {
            app.ui.grouping = app.ui.grouping.next();
            app.ui.scroll_offset = 0;
            return;
        }
        // Ksetra direct input
        (KeyCode::Char('k'), KeyModifiers::CONTROL) => {
            app.toggle_ksetra_input();
            return;
        }
        // Help
        (KeyCode::Char('?'), KeyModifiers::NONE) if !app.search.is_input_focused() => {
            app.toggle_help();
            return;
        }
        // Toggle focus with Tab
        (KeyCode::Tab, KeyModifiers::NONE) => {
            cycle_focus_forward(app);
            return;
        }
        (KeyCode::BackTab, _) => {
            cycle_focus_backward(app);
            return;
        }
        // Escape clears search or changes focus
        (KeyCode::Esc, KeyModifiers::NONE) => {
            if app.search.is_preview_focused() {
                app.search.focus = crate::state::FocusTarget::Results;
            } else if app.search.is_results_focused() {
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
    } else if app.search.is_results_focused() {
        handle_results_keys(app, key, modifiers);
    } else {
        handle_preview_keys(app, key, modifiers);
    }
}

/// Handle keys in Kriya-Suchi mode.
fn handle_kriya_suchi_keys(app: &mut AppState, key: KeyCode, modifiers: KeyModifiers) {
    match (key, modifiers) {
        (KeyCode::Esc, _) => app.toggle_kriya_suchi(),
        (KeyCode::Char('p'), KeyModifiers::CONTROL) => app.toggle_kriya_suchi(),
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => app.quit(),
        (KeyCode::Backspace, KeyModifiers::NONE) => {
            app.ui.kriya_suchi.pop_filter_char();
        }
        (KeyCode::Down, KeyModifiers::NONE) => {
            let actions = crate::kriya::filtered_kriyas(app);
            app.ui.kriya_suchi.select_next(actions.len());
        }
        (KeyCode::Up, KeyModifiers::NONE) => {
            let actions = crate::kriya::filtered_kriyas(app);
            app.ui.kriya_suchi.select_previous(actions.len());
        }
        (KeyCode::Char('j'), KeyModifiers::NONE) => {
            if app.ui.kriya_suchi.filter_query().is_empty() {
                let actions = crate::kriya::filtered_kriyas(app);
                app.ui.kriya_suchi.select_next(actions.len());
            } else {
                app.ui.kriya_suchi.push_filter_char('j');
            }
        }
        (KeyCode::Char('k'), KeyModifiers::NONE) => {
            if app.ui.kriya_suchi.filter_query().is_empty() {
                let actions = crate::kriya::filtered_kriyas(app);
                app.ui.kriya_suchi.select_previous(actions.len());
            } else {
                app.ui.kriya_suchi.push_filter_char('k');
            }
        }
        (KeyCode::Enter, KeyModifiers::NONE) => {
            let actions = crate::kriya::filtered_kriyas(app);
            let idx = app.ui.kriya_suchi.selected_index;
            if let Some(action) = actions.get(idx) {
                run_kriya_action(app, action.id);
            }
            app.toggle_kriya_suchi();
        }
        (KeyCode::Char(c), KeyModifiers::NONE) => {
            if !c.is_whitespace() {
                app.ui.kriya_suchi.push_filter_char(c);
            }
        }
        _ => {}
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
        // Page navigation
        (KeyCode::PageUp, KeyModifiers::NONE) | (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
            let step = app.ui.viewport_height.max(1) as isize;
            move_selection_by(app, -step);
        }
        (KeyCode::PageDown, KeyModifiers::NONE) | (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
            let step = app.ui.viewport_height.max(1) as isize;
            move_selection_by(app, step);
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
        // Ksetra navigation (scope stack)
        (KeyCode::Char('h'), KeyModifiers::NONE) | (KeyCode::Left, KeyModifiers::NONE) => {
            pop_ksetra(app);
        }
        (KeyCode::Char('l'), KeyModifiers::NONE) | (KeyCode::Right, KeyModifiers::NONE) => {
            if let Some(path) = app.search.selected_result().map(|r| r.path.clone()) {
                if is_dir(&path, app.view) {
                    push_ksetra(app, path);
                }
            }
        }
        // File actions
        (KeyCode::Enter, KeyModifiers::NONE) | (KeyCode::Char('o'), KeyModifiers::NONE) => {
            if let Some(path) = app.search.selected_result().map(|r| r.path.clone()) {
                if is_dir(&path, app.view) {
                    push_ksetra(app, path);
                } else {
                    open_in_editor(&path, app);
                }
            }
        }
        (KeyCode::Char('y'), KeyModifiers::NONE) => {
            if let Some(path) = app.search.selected_result().map(|r| r.path.clone()) {
                app.record_smriti_usage(path.clone(), SmritiAction::Copy);
                copy_to_clipboard(&path, app);
            }
        }
        (KeyCode::Char('p'), KeyModifiers::NONE) => {
            if let Some(path) = app.search.selected_result().map(|r| r.path.clone()) {
                app.record_smriti_usage(path.clone(), SmritiAction::Print);
                app.print_on_exit = Some(path);
                app.quit();
            }
        }
        (KeyCode::Char('r'), KeyModifiers::NONE) => {
            if let Some(path) = app.search.selected_result().map(|r| r.path.clone()) {
                app.record_smriti_usage(path.clone(), SmritiAction::Reveal);
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

/// Handle keys when preview is focused
fn handle_preview_keys(app: &mut AppState, key: KeyCode, modifiers: KeyModifiers) {
    match (key, modifiers) {
        (KeyCode::Char('/'), KeyModifiers::NONE) => {
            app.preview.start_search();
            app.mode = AppMode::PreviewSearch;
        }
        (KeyCode::Char('n'), KeyModifiers::NONE) => {
            jump_preview_match(app, 1, false);
        }
        (KeyCode::Char('N'), KeyModifiers::SHIFT) => {
            jump_preview_match(app, -1, false);
        }
        (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
            app.preview.toggle_line_numbers();
        }
        (KeyCode::Char('l'), KeyModifiers::CONTROL) => {
            app.preview.clear_search();
        }
        (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, KeyModifiers::NONE) => {
            scroll_preview_by(app, 1);
        }
        (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, KeyModifiers::NONE) => {
            scroll_preview_by(app, -1);
        }
        (KeyCode::PageDown, KeyModifiers::NONE) | (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
            let step = app.ui.preview_viewport_height.max(1) as i32;
            scroll_preview_by(app, step);
        }
        (KeyCode::PageUp, KeyModifiers::NONE) | (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
            let step = app.ui.preview_viewport_height.max(1) as i32;
            scroll_preview_by(app, -step);
        }
        (KeyCode::Char('g'), KeyModifiers::NONE) => {
            app.preview.scroll = 0;
        }
        (KeyCode::Char('G'), KeyModifiers::SHIFT) => {
            app.preview.scroll = preview_max_scroll(app);
        }
        _ => {}
    }
}

fn handle_preview_search_keys(app: &mut AppState, key: KeyCode, modifiers: KeyModifiers) {
    match (key, modifiers) {
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
            app.quit();
        }
        (KeyCode::Esc, _) => {
            app.preview.cancel_search();
            app.mode = AppMode::Search;
        }
        (KeyCode::Enter, KeyModifiers::NONE) => {
            app.preview.apply_search();
            app.mode = AppMode::Search;
            if !app.preview.search_query.is_empty() {
                jump_preview_match(app, 1, true);
            }
        }
        (KeyCode::Backspace, KeyModifiers::NONE) => {
            app.preview.delete_search_char();
        }
        (KeyCode::Left, KeyModifiers::NONE) => {
            app.preview.move_search_cursor_left();
        }
        (KeyCode::Right, KeyModifiers::NONE) => {
            app.preview.move_search_cursor_right();
        }
        (KeyCode::Char(c), KeyModifiers::NONE) | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
            app.preview.insert_search_char(c);
        }
        _ => {}
    }
}

/// Handle keys in ksetra input mode.
fn handle_ksetra_input_keys(app: &mut AppState, key: KeyCode, modifiers: KeyModifiers) {
    match (key, modifiers) {
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
            app.quit();
        }
        (KeyCode::Esc, _) | (KeyCode::Char('k'), KeyModifiers::CONTROL) => {
            app.toggle_ksetra_input();
        }
        (KeyCode::Enter, KeyModifiers::NONE) => {
            // Validate and apply the ksetra
            let path = app.ksetra_input.expand_path();
            if path.is_dir() {
                // Clear ksetra stack and set to this path
                while app.ksetra.pop().is_some() {}
                app.ksetra.push(path);
                app.clear_results();
                app.preview.clear();
                app.toggle_ksetra_input();
            } else {
                app.ksetra_input.error = Some("Not a directory".to_string());
            }
        }
        (KeyCode::Tab, KeyModifiers::NONE) => {
            // Tab completion: if completions available, cycle through them
            if app.ksetra_input.completions.is_empty() {
                // Trigger completion search
                trigger_ksetra_completion(app);
            } else {
                // Cycle to next completion and apply
                app.ksetra_input.select_next_completion();
                app.ksetra_input.apply_completion();
            }
        }
        (KeyCode::BackTab, _) => {
            // Shift+Tab: cycle backwards
            if !app.ksetra_input.completions.is_empty() {
                app.ksetra_input.select_previous_completion();
                app.ksetra_input.apply_completion();
            }
        }
        (KeyCode::Backspace, KeyModifiers::NONE) => {
            app.ksetra_input.pop_char();
            app.ksetra_input.completions.clear();
        }
        (KeyCode::Delete, KeyModifiers::NONE) => {
            app.ksetra_input.delete_char();
            app.ksetra_input.completions.clear();
        }
        (KeyCode::Left, KeyModifiers::NONE) => {
            app.ksetra_input.move_cursor_left();
        }
        (KeyCode::Right, KeyModifiers::NONE) => {
            app.ksetra_input.move_cursor_right();
        }
        (KeyCode::Home, KeyModifiers::NONE) | (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
            app.ksetra_input.move_cursor_start();
        }
        (KeyCode::End, KeyModifiers::NONE) | (KeyCode::Char('e'), KeyModifiers::CONTROL) => {
            app.ksetra_input.move_cursor_end();
        }
        (KeyCode::Down, KeyModifiers::NONE) | (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
            if !app.ksetra_input.completions.is_empty() {
                app.ksetra_input.select_next_completion();
            }
        }
        (KeyCode::Up, KeyModifiers::NONE) | (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
            if !app.ksetra_input.completions.is_empty() {
                app.ksetra_input.select_previous_completion();
            }
        }
        (KeyCode::Char(c), KeyModifiers::NONE) | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
            app.ksetra_input.push_char(c);
            app.ksetra_input.completions.clear();
        }
        _ => {}
    }
}

/// Trigger directory completion for ksetra input.
fn trigger_ksetra_completion(app: &mut AppState) {
    use crate::state::KsetraInputState;

    let input = app.ksetra_input.input.trim();
    if input.is_empty() {
        return;
    }

    // Expand ~ to home directory for filesystem operations
    let expanded = app.ksetra_input.expand_path();

    // Find the parent directory and prefix to match
    let (parent, prefix) = if expanded.is_dir() && input.ends_with('/') {
        (expanded.clone(), String::new())
    } else if let Some(parent) = expanded.parent() {
        let prefix = expanded
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();
        (parent.to_path_buf(), prefix)
    } else {
        return;
    };

    // Read directory entries and filter
    let Ok(entries) = std::fs::read_dir(&parent) else {
        return;
    };

    let mut completions: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .filter(|e| {
            if prefix.is_empty() {
                true
            } else {
                e.file_name()
                    .to_str()
                    .map(|n| n.to_lowercase().starts_with(&prefix))
                    .unwrap_or(false)
            }
        })
        .map(|e| {
            let path = e.path();
            // Convert back to display format (with ~)
            KsetraInputState::display_path(&path)
        })
        .take(10)
        .collect();

    completions.sort();
    app.ksetra_input.set_completions(completions);
}

fn cycle_focus_forward(app: &mut AppState) {
    let has_preview = app.preview.is_visible;
    app.search.focus = match app.search.focus {
        crate::state::FocusTarget::Input => crate::state::FocusTarget::Results,
        crate::state::FocusTarget::Results => {
            if has_preview {
                crate::state::FocusTarget::Preview
            } else {
                crate::state::FocusTarget::Input
            }
        }
        crate::state::FocusTarget::Preview => crate::state::FocusTarget::Input,
    };
}

fn cycle_focus_backward(app: &mut AppState) {
    let has_preview = app.preview.is_visible;
    app.search.focus = match app.search.focus {
        crate::state::FocusTarget::Input => {
            if has_preview {
                crate::state::FocusTarget::Preview
            } else {
                crate::state::FocusTarget::Results
            }
        }
        crate::state::FocusTarget::Results => crate::state::FocusTarget::Input,
        crate::state::FocusTarget::Preview => crate::state::FocusTarget::Results,
    };
}

fn move_selection_by(app: &mut AppState, delta: isize) {
    if app.search.results.is_empty() {
        return;
    }

    let len = app.search.results.len() as isize;
    let current = app.search.selected_index as isize;
    let next = (current + delta).clamp(0, len - 1);
    app.search.selected_index = next as usize;
}

fn preview_max_scroll(app: &AppState) -> u16 {
    let viewport = app.ui.preview_viewport_height.max(1);
    let max_start = app.preview.lines.len().saturating_sub(viewport);
    max_start.min(u16::MAX as usize) as u16
}

fn scroll_preview_by(app: &mut AppState, delta: i32) {
    let current = app.preview.scroll as i32;
    let max = preview_max_scroll(app) as i32;
    let next = (current + delta).clamp(0, max);
    app.preview.scroll = next as u16;
}

fn jump_preview_match(app: &mut AppState, direction: i32, include_current: bool) {
    let needle = if app.mode == AppMode::PreviewSearch {
        app.preview.search_input.trim().to_string()
    } else {
        app.preview.search_query.trim().to_string()
    };

    if needle.is_empty() || app.preview.lines.is_empty() {
        return;
    }

    let len = app.preview.lines.len();
    let start = app.preview.scroll as usize;
    let start = start.min(len.saturating_sub(1));
    let start = if include_current {
        start
    } else if direction >= 0 {
        (start + 1) % len
    } else if start == 0 {
        len.saturating_sub(1)
    } else {
        start - 1
    };

    let needle_ascii_lower = needle.is_ascii().then(|| needle.to_ascii_lowercase());
    let needle_ascii_lower = needle_ascii_lower.as_deref();

    let Some(line_idx) = find_next_match_line(
        &app.preview.lines,
        &needle,
        needle_ascii_lower,
        start,
        direction,
    ) else {
        app.error = Some("no preview matches".to_string());
        return;
    };

    let viewport = app.ui.preview_viewport_height.max(1);
    let max_scroll = preview_max_scroll(app);
    let target = line_idx.saturating_sub(viewport / 2);
    app.preview.scroll = (target.min(max_scroll as usize)).min(u16::MAX as usize) as u16;
}

fn find_next_match_line(
    lines: &[crate::state::StyledLine],
    needle: &str,
    needle_ascii_lower: Option<&str>,
    start_line: usize,
    direction: i32,
) -> Option<usize> {
    let len = lines.len();
    if len == 0 {
        return None;
    }

    let contains = |idx: usize| -> bool {
        let mut text = String::new();
        for seg in &lines[idx] {
            text.push_str(seg.text.as_str());
        }
        if let Some(needle_ascii_lower) = needle_ascii_lower {
            if text.is_ascii() {
                return text.to_ascii_lowercase().contains(needle_ascii_lower);
            }
        }
        text.contains(needle)
    };

    let mut idx = start_line.min(len.saturating_sub(1));
    for _ in 0..len {
        if contains(idx) {
            return Some(idx);
        }

        if direction >= 0 {
            idx = (idx + 1) % len;
        } else if idx == 0 {
            idx = len.saturating_sub(1);
        } else {
            idx -= 1;
        }
    }

    None
}

/// Open file in $EDITOR or fallback editor
fn open_in_editor(path: &str, app: &mut AppState) {
    // Store path to open after TUI exits
    app.record_smriti_usage(path.to_string(), SmritiAction::Open);
    app.open_in_editor = Some(path.to_string());
    app.quit();
}

fn is_dir(path: &str, view: crate::state::ViewKind) -> bool {
    if view == crate::state::ViewKind::Sthana {
        return true;
    }

    std::fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false)
}

fn push_ksetra(app: &mut AppState, path: String) {
    app.record_smriti_usage(path.clone(), SmritiAction::Enter);
    app.ksetra.push(std::path::PathBuf::from(path));
    app.clear_results();
    app.preview.clear();
    app.ui.scroll_offset = 0;
    app.search.is_searching = true;
}

fn pop_ksetra(app: &mut AppState) {
    if app.ksetra.pop().is_some() {
        app.clear_results();
        app.preview.clear();
        app.ui.scroll_offset = 0;
        app.search.is_searching = true;
    } else {
        app.error = Some("ksetra is already global".to_string());
    }
}

fn run_kriya_action(app: &mut AppState, id: crate::kriya::KriyaId) {
    use crate::kriya::KriyaId;

    match id {
        KriyaId::OpenOrEnter => {
            if let Some(path) = app.search.selected_result().map(|r| r.path.clone()) {
                if is_dir(&path, app.view) {
                    push_ksetra(app, path);
                } else {
                    open_in_editor(&path, app);
                }
            }
        }
        KriyaId::CopyPath => {
            if let Some(path) = app.search.selected_result().map(|r| r.path.clone()) {
                app.record_smriti_usage(path.clone(), SmritiAction::Copy);
                copy_to_clipboard(&path, app);
            }
        }
        KriyaId::Reveal => {
            if let Some(path) = app.search.selected_result().map(|r| r.path.clone()) {
                app.record_smriti_usage(path.clone(), SmritiAction::Reveal);
                reveal_in_finder(&path, app);
            }
        }
        KriyaId::PrintPath => {
            if let Some(path) = app.search.selected_result().map(|r| r.path.clone()) {
                app.record_smriti_usage(path.clone(), SmritiAction::Print);
                app.print_on_exit = Some(path);
                app.quit();
            }
        }
        KriyaId::ForgetSmriti => {
            if let Some(path) = app.search.selected_result().map(|r| r.path.clone()) {
                app.forget_smriti_path(path.clone());
                app.search.results.retain(|result| result.path != path);
                app.search.clamp_selection();
                app.error = Some(format!("✓ Forgot from smriti: {}", path));
            }
        }
        KriyaId::TogglePreview => {
            app.preview.toggle();
            if !app.preview.is_visible && app.search.is_preview_focused() {
                app.search.focus = crate::state::FocusTarget::Results;
            }
        }
        KriyaId::ToggleGrouping => {
            app.ui.grouping = app.ui.grouping.next();
            app.ui.scroll_offset = 0;
        }
        KriyaId::PopKsetra => {
            pop_ksetra(app);
        }
        KriyaId::SetKsetra => {
            app.toggle_ksetra_input();
        }
        KriyaId::TogglePreviewLineNumbers => {
            app.preview.toggle_line_numbers();
        }
        KriyaId::ClearPreviewSearch => {
            app.preview.clear_search();
        }
        KriyaId::Quit => {
            app.quit();
        }
    }
}

fn trigger_search(
    cmd_tx: &mpsc::Sender<WorkerCommand>,
    app: &mut AppState,
    search_id: &mut u64,
    active_search_id: &mut u64,
    last_search_sent_at: &mut std::time::Instant,
) -> bool {
    let parsed = crate::state::parse_query(&app.search.query);

    *search_id = (*search_id).wrapping_add(1);
    *active_search_id = *search_id;
    let command = WorkerCommand::Search {
        id: *active_search_id,
        query: parsed.term,
        limit: 100,
        view: app.view,
        boost_scope: app.ksetra.current().cloned(),
        filter_scope: app.ksetra.current().cloned(),
        niyamas: parsed.niyamas,
    };

    if cmd_tx.send(command).is_err() {
        app.search.is_searching = false;
        app.error = Some("Search worker is unavailable".to_string());
        return false;
    }

    app.search.is_searching = true;
    *last_search_sent_at = std::time::Instant::now();
    true
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
        AppMode::KriyaSuchi => {
            render_search(f, app);
            ui::overlays::render_kriya_suchi(f, app);
        }
        AppMode::PreviewSearch => {
            render_search(f, app);
            ui::overlays::render_preview_search(f, app);
        }
        AppMode::KsetraInput => {
            render_search(f, app);
            ui::overlays::render_ksetra_input(f, app);
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
            Constraint::Length(4), // Search input + niyamas
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
        app.ui.preview_viewport_height = body[1].height.saturating_sub(2) as usize;
        ui::results::render(f, body[0], app);
        ui::preview::render(f, body[1], app);
    } else {
        app.ui.preview_viewport_height = 0;
        ui::results::render(f, chunks[2], app);
    }

    ui::footer::render(f, chunks[3], app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{FocusTarget, StyledLine, StyledSegment, TextKind, TextStyle, ViewKind};
    use ratatui::{backend::TestBackend, Terminal};
    use vicaya_core::ipc::BuildInfo;
    use vicaya_index::SearchResult;

    fn search_result(path: &std::path::Path, name: &str, size: u64) -> SearchResult {
        SearchResult {
            path: path.to_string_lossy().to_string(),
            name: name.to_string(),
            score: 0.92,
            size,
            mtime: 1_700_000_000,
        }
    }

    fn plain_line(text: &str) -> StyledLine {
        vec![StyledSegment {
            text: text.to_string(),
            style: TextStyle::default(),
        }]
    }

    fn meta_line(text: &str) -> StyledLine {
        vec![StyledSegment {
            text: text.to_string(),
            style: TextStyle {
                kind: TextKind::Meta,
                ..Default::default()
            },
        }]
    }

    fn buffer_text(app: &mut AppState, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| ui_render(f, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .chunks(width as usize)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn apply_sample_status(app: &mut AppState) {
        app.daemon_status = Some(crate::client::DaemonStatus {
            build: BuildInfo {
                version: "1.2.0".to_string(),
                git_sha: "abc1234".to_string(),
                timestamp: "2026-05-19T00:00:00Z".to_string(),
                target: "aarch64-apple-darwin".to_string(),
            },
            indexed_files: 1_234_567,
            trigram_count: 98_765,
            arena_size: 4_096,
            last_updated: 1_700_000_000,
            reconciling: true,
        });
    }

    #[test]
    fn search_mode_keys_cover_query_focus_preview_and_selection_actions() {
        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("src");
        std::fs::create_dir_all(&subdir).unwrap();
        let file = dir.path().join("README.md");
        std::fs::write(&file, "readme").unwrap();

        let mut app = AppState::new();
        app.search.set_results(vec![
            search_result(&subdir, "src", 0),
            search_result(&file, "README.md", 6),
        ]);
        app.ui.viewport_height = 1;

        handle_key_event(&mut app, KeyCode::Char('c'), KeyModifiers::NONE);
        handle_key_event(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);
        handle_key_event(&mut app, KeyCode::Char('r'), KeyModifiers::NONE);
        handle_key_event(&mut app, KeyCode::Left, KeyModifiers::NONE);
        handle_key_event(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(app.search.query, "cr");

        handle_key_event(&mut app, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(app.search.focus, FocusTarget::Results);
        handle_key_event(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(app.search.selected_index, 1);
        handle_key_event(&mut app, KeyCode::PageUp, KeyModifiers::NONE);
        assert_eq!(app.search.selected_index, 0);
        handle_key_event(&mut app, KeyCode::Char('G'), KeyModifiers::SHIFT);
        assert_eq!(app.search.selected_index, 1);
        handle_key_event(&mut app, KeyCode::Char('g'), KeyModifiers::NONE);
        assert_eq!(app.search.selected_index, 0);

        handle_key_event(&mut app, KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(app.ksetra.current(), Some(&subdir));
        assert!(app.search.is_searching);
        assert!(app.search.results.is_empty());

        app.search
            .set_results(vec![search_result(&file, "README.md", 6)]);
        app.search.focus = FocusTarget::Results;
        handle_key_event(&mut app, KeyCode::Char('p'), KeyModifiers::NONE);
        assert_eq!(app.print_on_exit, Some(file.to_string_lossy().to_string()));
        assert!(app.should_quit());
    }

    #[test]
    fn content_result_anchor_parses_line_from_result_name() {
        let result = SearchResult {
            path: "/tmp/project/src/main.rs".to_string(),
            name: "main.rs:42:7  fn main() {}".to_string(),
            score: 1.0,
            size: 0,
            mtime: 0,
        };

        assert_eq!(
            content_result_anchor(ViewKind::Antarvicaya, &result),
            Some(42)
        );
        assert_eq!(content_result_anchor(ViewKind::Patra, &result), None);
    }

    #[test]
    fn preview_scroll_for_line_keeps_match_near_top() {
        let mut app = AppState::new();
        app.preview.lines = vec![
            meta_line("/tmp/file.rs"),
            meta_line("10 bytes"),
            meta_line(""),
            plain_line("one"),
            plain_line("two"),
            plain_line("three"),
            plain_line("four"),
        ];
        app.preview.content_line_numbers =
            crate::state::compute_content_line_numbers(&app.preview.lines);

        assert_eq!(preview_scroll_for_line(&app, 4), 3);
    }

    #[test]
    fn global_search_keys_toggle_modes_and_preview_focus_safely() {
        let mut app = AppState::new();
        app.search.focus = FocusTarget::Results;

        handle_key_event(&mut app, KeyCode::Char('?'), KeyModifiers::NONE);
        assert_eq!(app.mode, AppMode::Help);
        handle_key_event(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.mode, AppMode::Search);

        handle_key_event(&mut app, KeyCode::Char('t'), KeyModifiers::CONTROL);
        assert_eq!(app.mode, AppMode::DrishtiSwitcher);
        handle_key_event(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.mode, AppMode::Search);

        handle_key_event(&mut app, KeyCode::Char('p'), KeyModifiers::CONTROL);
        assert_eq!(app.mode, AppMode::KriyaSuchi);
        handle_key_event(&mut app, KeyCode::Char('p'), KeyModifiers::CONTROL);
        assert_eq!(app.mode, AppMode::Search);

        handle_key_event(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.search.focus, FocusTarget::Preview);
        handle_key_event(&mut app, KeyCode::Char('o'), KeyModifiers::CONTROL);
        assert!(!app.preview.is_visible);
        assert_eq!(app.search.focus, FocusTarget::Results);
        handle_key_event(&mut app, KeyCode::BackTab, KeyModifiers::SHIFT);
        assert_eq!(app.search.focus, FocusTarget::Input);
    }

    #[test]
    fn narrow_terminal_control_overlays_do_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = AppState::with_startup_scope(Some(dir.path().to_path_buf()));

        handle_key_event(&mut app, KeyCode::Char('k'), KeyModifiers::CONTROL);
        assert_eq!(app.mode, AppMode::KsetraInput);
        let ksetra = buffer_text(&mut app, 48, 24);
        assert!(ksetra.contains("ksetra"));

        handle_key_event(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        app.search.focus = FocusTarget::Preview;
        handle_key_event(&mut app, KeyCode::Char('/'), KeyModifiers::NONE);
        assert_eq!(app.mode, AppMode::PreviewSearch);
        let preview = buffer_text(&mut app, 42, 20);
        assert!(preview.contains("preview"));
    }

    #[test]
    fn drishti_switcher_selects_enabled_views_and_reports_disabled_views() {
        let mut app = AppState::new();

        app.toggle_drishti_switcher();
        for ch in "sth".chars() {
            handle_key_event(&mut app, KeyCode::Char(ch), KeyModifiers::NONE);
        }
        handle_key_event(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.view, ViewKind::Sthana);
        assert_eq!(app.mode, AppMode::Search);

        app.toggle_drishti_switcher();
        for ch in "git".chars() {
            handle_key_event(&mut app, KeyCode::Char(ch), KeyModifiers::NONE);
        }
        handle_key_event(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.view, ViewKind::Sthana);
        assert_eq!(app.mode, AppMode::Search);
        assert!(app
            .error
            .as_deref()
            .is_some_and(|msg| msg.contains("coming soon")));
    }

    #[test]
    fn kriya_actions_cover_safe_stateful_commands() {
        let dir = tempfile::tempdir().unwrap();
        let selected = dir.path().join("selected.txt");
        std::fs::write(&selected, "selected").unwrap();

        let mut app = AppState::new();
        app.ksetra.push(dir.path().to_path_buf());
        app.search
            .set_results(vec![search_result(&selected, "selected.txt", 8)]);
        app.preview.lines = vec![plain_line("needle"), plain_line("haystack")];
        app.preview.search_query = "needle".to_string();

        run_kriya_action(&mut app, crate::kriya::KriyaId::TogglePreview);
        assert!(!app.preview.is_visible);
        run_kriya_action(&mut app, crate::kriya::KriyaId::ToggleGrouping);
        assert_eq!(app.ui.grouping.label(), "dir");
        run_kriya_action(&mut app, crate::kriya::KriyaId::PopKsetra);
        assert!(app.ksetra.is_global());
        run_kriya_action(&mut app, crate::kriya::KriyaId::SetKsetra);
        assert_eq!(app.mode, AppMode::KsetraInput);

        app.mode = AppMode::Search;
        run_kriya_action(&mut app, crate::kriya::KriyaId::TogglePreviewLineNumbers);
        assert!(app.preview.show_line_numbers);
        run_kriya_action(&mut app, crate::kriya::KriyaId::ClearPreviewSearch);
        assert!(app.preview.search_query.is_empty());
        app.search
            .set_results(vec![search_result(&selected, "selected.txt", 8)]);
        run_kriya_action(&mut app, crate::kriya::KriyaId::PrintPath);
        assert_eq!(
            app.print_on_exit,
            Some(selected.to_string_lossy().to_string())
        );
        assert!(app.should_quit());
    }

    #[test]
    fn ksetra_input_validates_completes_and_applies_scope() {
        let dir = tempfile::tempdir().unwrap();
        let alpha = dir.path().join("alpha");
        let beta = dir.path().join("beta");
        std::fs::create_dir_all(&alpha).unwrap();
        std::fs::create_dir_all(&beta).unwrap();
        std::fs::write(dir.path().join("alpha.txt"), "").unwrap();

        let mut app = AppState::new();
        app.toggle_ksetra_input();
        for ch in dir.path().join("a").to_string_lossy().chars() {
            handle_key_event(&mut app, KeyCode::Char(ch), KeyModifiers::NONE);
        }
        handle_key_event(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.ksetra_input.completions.len(), 1);
        handle_key_event(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.ksetra_input.input, alpha.to_string_lossy());
        assert!(app.ksetra_input.completions.is_empty());

        handle_key_event(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.mode, AppMode::Search);
        assert_eq!(app.ksetra.current(), Some(&alpha));

        app.toggle_ksetra_input();
        for ch in dir.path().join("missing").to_string_lossy().chars() {
            handle_key_event(&mut app, KeyCode::Char(ch), KeyModifiers::NONE);
        }
        handle_key_event(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.ksetra_input.error.as_deref(), Some("Not a directory"));
    }

    #[test]
    fn preview_search_and_scrolling_keep_matches_visible() {
        let mut app = AppState::new();
        app.search.focus = FocusTarget::Preview;
        app.ui.preview_viewport_height = 3;
        app.preview.lines = vec![
            meta_line("meta"),
            plain_line("alpha"),
            plain_line("beta"),
            plain_line("gamma needle"),
            plain_line("delta"),
            plain_line("needle again"),
        ];
        app.preview.content_line_numbers =
            crate::state::compute_content_line_numbers(&app.preview.lines);

        handle_key_event(&mut app, KeyCode::Char('/'), KeyModifiers::NONE);
        assert_eq!(app.mode, AppMode::PreviewSearch);
        for ch in "needle".chars() {
            handle_key_event(&mut app, KeyCode::Char(ch), KeyModifiers::NONE);
        }
        handle_key_event(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.mode, AppMode::Search);
        assert_eq!(app.preview.search_query, "needle");
        assert!(app.preview.scroll > 0);

        let first_scroll = app.preview.scroll;
        handle_key_event(&mut app, KeyCode::Char('n'), KeyModifiers::NONE);
        assert!(app.preview.scroll >= first_scroll);
        handle_key_event(&mut app, KeyCode::Char('G'), KeyModifiers::SHIFT);
        assert_eq!(app.preview.scroll, 3);
        handle_key_event(&mut app, KeyCode::PageUp, KeyModifiers::NONE);
        assert_eq!(app.preview.scroll, 0);
        handle_key_event(&mut app, KeyCode::Char('l'), KeyModifiers::CONTROL);
        assert!(app.preview.search_query.is_empty());
    }

    #[test]
    fn trigger_search_parses_niyamas_and_scopes_worker_command() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = AppState::with_startup_scope(Some(dir.path().to_path_buf()));
        app.search.set_query("main ext:rs type:file".to_string());
        let (tx, rx) = mpsc::channel();
        let mut search_id = 0;
        let mut active_search_id = 0;
        let mut last = std::time::Instant::now();

        assert!(trigger_search(
            &tx,
            &mut app,
            &mut search_id,
            &mut active_search_id,
            &mut last,
        ));

        match rx.try_recv().unwrap() {
            WorkerCommand::Search {
                id,
                query,
                limit,
                boost_scope,
                filter_scope,
                niyamas,
                ..
            } => {
                assert_eq!(id, 1);
                assert_eq!(query, "main");
                assert_eq!(limit, 100);
                assert_eq!(boost_scope.as_deref(), Some(dir.path()));
                assert_eq!(filter_scope.as_deref(), Some(dir.path()));
                assert_eq!(niyamas.len(), 2);
            }
            _ => panic!("expected search command"),
        }
        assert_eq!(search_id, 1);
        assert_eq!(active_search_id, 1);
        assert!(app.search.is_searching);
    }

    #[test]
    fn trigger_search_does_not_leave_tui_stuck_when_worker_is_gone() {
        let (tx, rx) = mpsc::channel();
        drop(rx);
        let mut app = AppState::new();
        app.search.set_query("record".to_string());
        let mut search_id = 0;
        let mut active_search_id = 0;
        let mut last = std::time::Instant::now();

        assert!(!trigger_search(
            &tx,
            &mut app,
            &mut search_id,
            &mut active_search_id,
            &mut last,
        ));

        assert!(!app.search.is_searching);
        assert_eq!(app.error.as_deref(), Some("Search worker is unavailable"));
    }

    #[test]
    fn render_search_and_overlays_expose_real_tui_surfaces() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Cargo.toml");
        std::fs::write(&file, "[package]\nname = \"demo\"\n").unwrap();
        let mut app = AppState::with_startup_scope(Some(dir.path().to_path_buf()));
        apply_sample_status(&mut app);
        app.search.set_query("cargo".to_string());
        app.search
            .set_results(vec![search_result(&file, "Cargo.toml", 24)]);
        app.preview.title = "Cargo.toml".to_string();
        app.preview.path = Some(file.to_string_lossy().to_string());
        app.preview.lines = vec![
            meta_line(file.to_string_lossy().as_ref()),
            plain_line("name = demo"),
        ];
        app.preview.content_line_numbers =
            crate::state::compute_content_line_numbers(&app.preview.lines);

        let screen = buffer_text(&mut app, 120, 30);
        assert!(screen.contains("vicaya"));
        assert!(screen.contains("prashna: cargo"));
        assert!(screen.contains("Cargo.toml"));
        assert!(screen.contains("purvadarshana"));

        app.mode = AppMode::Help;
        assert!(buffer_text(&mut app, 100, 28).contains("Help"));

        app.mode = AppMode::DrishtiSwitcher;
        app.ui.drishti_switcher.push_filter_char('s');
        assert!(buffer_text(&mut app, 100, 28).contains("Sthana"));

        app.mode = AppMode::KriyaSuchi;
        app.ui.kriya_suchi.push_filter_char('p');
        let kriya = buffer_text(&mut app, 100, 28);
        assert!(kriya.contains("Print path") || kriya.contains("purvadarshana"));

        app.mode = AppMode::PreviewSearch;
        app.preview.start_search();
        app.preview.insert_search_char('d');
        assert!(buffer_text(&mut app, 100, 28).contains("preview"));
        assert!(buffer_text(&mut app, 42, 20).contains("preview"));

        app.mode = AppMode::KsetraInput;
        app.ksetra_input.input = dir.path().to_string_lossy().to_string();
        app.ksetra_input.cursor = app.ksetra_input.input.len();
        app.ksetra_input
            .set_completions(vec![dir.path().to_string_lossy().to_string()]);
        assert!(buffer_text(&mut app, 100, 28).contains("ksetra"));
        assert!(buffer_text(&mut app, 48, 24).contains("ksetra"));

        app.mode = AppMode::Confirm(crate::state::Action::Quit);
        assert!(buffer_text(&mut app, 100, 28).contains("sure"));
    }

    #[test]
    fn result_rendering_covers_grouping_scrolling_and_hidden_preview() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        let tests = dir.path().join("tests");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&tests).unwrap();
        let main_rs = src.join("main.rs");
        let lib_rs = src.join("lib.rs");
        let smoke_md = tests.join("smoke.md");
        std::fs::write(&main_rs, "fn main() {}\n").unwrap();
        std::fs::write(&lib_rs, "pub fn lib() {}\n").unwrap();
        std::fs::write(&smoke_md, "# smoke\n").unwrap();

        let mut app = AppState::new();
        app.preview.is_visible = false;
        app.search.set_query("src ext:rs".to_string());
        app.search.set_results(vec![
            search_result(&main_rs, "main.rs", 12),
            search_result(&lib_rs, "lib.rs", 14),
            search_result(&smoke_md, "smoke.md", 8),
        ]);
        app.search.focus = FocusTarget::Results;
        app.search.selected_index = 2;
        app.ui.viewport_height = 2;
        app.ui.grouping = crate::state::GroupingMode::Directory;

        let directory_screen = buffer_text(&mut app, 90, 16);
        assert!(directory_screen.contains("src"));
        assert!(directory_screen.contains("tests"));

        app.ui.grouping = crate::state::GroupingMode::Extension;
        app.ui.scroll_offset = 0;
        let extension_screen = buffer_text(&mut app, 90, 16);
        assert!(extension_screen.contains(".rs"));
        assert!(extension_screen.contains(".md"));
        assert!(extension_screen.contains("varga:ext"));
    }
}
