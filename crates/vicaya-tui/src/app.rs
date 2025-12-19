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
    let mut last_ksetra = app.ksetra.current().cloned();
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
                        app.preview.content_line_numbers =
                            crate::state::compute_content_line_numbers(&app.preview.lines);
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
                if last_preview_path.as_deref() != Some(result.path.as_str()) {
                    preview_id = preview_id.wrapping_add(1);
                    active_preview_id = preview_id;
                    last_preview_path = Some(result.path.clone());
                    app.preview.is_loading = true;
                    app.preview.truncated = false;
                    app.preview.path = Some(result.path.clone());
                    app.preview.title = result.name.clone();
                    app.preview.lines.clear();
                    app.preview.content_line_numbers.clear();
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
        AppMode::PreviewSearch => handle_preview_search_keys(app, key, modifiers),
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
            if app.ksetra.pop().is_some() {
                app.clear_results();
                app.preview.clear();
                app.ui.scroll_offset = 0;
                app.search.is_searching = true;
            } else {
                app.error = Some("ksetra is already global".to_string());
            }
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
    app.ksetra.push(std::path::PathBuf::from(path));
    app.clear_results();
    app.preview.clear();
    app.ui.scroll_offset = 0;
    app.search.is_searching = true;
}

fn trigger_search(
    cmd_tx: &mpsc::Sender<WorkerCommand>,
    app: &mut AppState,
    search_id: &mut u64,
    active_search_id: &mut u64,
    last_search_sent_at: &mut std::time::Instant,
) {
    let parsed = crate::state::parse_query(&app.search.query);

    app.search.is_searching = true;
    *search_id = (*search_id).wrapping_add(1);
    *active_search_id = *search_id;
    let _ = cmd_tx.send(WorkerCommand::Search {
        id: *active_search_id,
        query: parsed.term,
        limit: 100,
        view: app.view,
        scope: app.ksetra.current().cloned(),
        niyamas: parsed.niyamas,
    });
    *last_search_sent_at = std::time::Instant::now();
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
        AppMode::PreviewSearch => {
            render_search(f, app);
            ui::overlays::render_preview_search(f, app);
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
