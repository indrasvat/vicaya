//! Results list rendering.

use crate::state::AppState;
use crate::ui;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
    Frame,
};

#[derive(Debug, Clone)]
enum RenderRow {
    Header(String),
    Result(usize),
}

pub fn render(f: &mut Frame, area: Rect, app: &mut AppState) {
    let results = &app.search.results;
    let selected = app.search.selected_index;

    let (rows, selected_row) = build_rows(app);

    // Update scrolling state.
    let viewport_height = area.height.saturating_sub(2) as usize; // borders
    app.ui.viewport_height = viewport_height.max(1);
    app.ui.update_scroll(selected_row, rows.len());

    let start = app.ui.scroll_offset.min(rows.len());
    let end = (start + viewport_height).min(rows.len());

    // Calculate available width for path display (rough estimate)
    let available_width = area.width.saturating_sub(4); // Account for borders
    let max_path_len = available_width.saturating_sub(30) as usize; // Reserve space for name, score, marker

    let items: Vec<ListItem> = rows[start..end]
        .iter()
        .map(|row| match row {
            RenderRow::Header(label) => {
                let line = Line::from(vec![
                    Span::styled("┈ ", Style::default().fg(ui::TEXT_MUTED)),
                    Span::styled(
                        label,
                        Style::default()
                            .fg(ui::TEXT_SECONDARY)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]);
                ListItem::new(line).style(Style::default())
            }
            RenderRow::Result(result_index) => {
                let result = &results[*result_index];
                let marker = if *result_index == selected {
                    "▸"
                } else {
                    " "
                };
                let score_color = ui::score_color(result.score);
                let is_selected = *result_index == selected;

                let path = std::path::Path::new(&result.path);
                let dir_path = path.parent().and_then(|p| p.to_str()).unwrap_or("");

                // Truncate path if not selected
                let display_path = truncate_path(dir_path, max_path_len.max(30), is_selected);

                let mut spans = vec![
                    Span::styled(marker, Style::default().fg(ui::PRIMARY)),
                    Span::raw(" "),
                ];

                let (name, name_style) = if app.view == crate::state::ViewKind::Sthana {
                    (format!("{}/", result.name), Style::default().fg(ui::ACCENT))
                } else {
                    (result.name.clone(), Style::default().fg(ui::TEXT_PRIMARY))
                };

                spans.extend(vec![
                    Span::styled(name, name_style),
                    Span::raw(" "),
                    Span::styled(
                        format!("({}) ", display_path),
                        Style::default().fg(ui::TEXT_MUTED),
                    ),
                    Span::styled(
                        format!("{:.2}", result.score),
                        Style::default().fg(score_color),
                    ),
                ]);

                let line = Line::from(spans);
                let style = if is_selected {
                    Style::default().bg(ui::BG_ELEVATED)
                } else {
                    Style::default()
                };

                ListItem::new(line).style(style)
            }
        })
        .collect();

    let border_style = if app.search.is_results_focused() {
        Style::default().fg(ui::BORDER_FOCUS)
    } else {
        Style::default().fg(ui::BORDER_DIM)
    };

    let title = if app.search.is_searching {
        format!(
            "phala ({})  searching…  varga:{}",
            results.len(),
            app.ui.grouping.label()
        )
    } else {
        format!(
            "phala ({})  varga:{}",
            results.len(),
            app.ui.grouping.label()
        )
    };

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title)
            .style(Style::default().bg(ui::BG_SURFACE)),
    );

    f.render_widget(list.style(Style::default().bg(ui::BG_SURFACE)), area);
}

fn build_rows(app: &AppState) -> (Vec<RenderRow>, usize) {
    let results = &app.search.results;
    if results.is_empty() {
        return (Vec::new(), 0);
    }

    let mut rows: Vec<RenderRow> = Vec::new();
    let mut selected_row: usize = 0;
    let mut last_group: Option<String> = None;
    let scope = app.ksetra.current().map(|p| p.as_path());

    for (idx, result) in results.iter().enumerate() {
        if app.ui.grouping != crate::state::GroupingMode::None {
            let group = match app.ui.grouping {
                crate::state::GroupingMode::None => String::new(),
                crate::state::GroupingMode::Directory => directory_group_label(result, scope),
                crate::state::GroupingMode::Extension => extension_group_label(result),
            };

            if last_group.as_deref() != Some(group.as_str()) {
                rows.push(RenderRow::Header(group.clone()));
                last_group = Some(group);
            }
        }

        rows.push(RenderRow::Result(idx));
        if idx == app.search.selected_index {
            selected_row = rows.len().saturating_sub(1);
        }
    }

    (rows, selected_row)
}

fn directory_group_label(
    result: &vicaya_index::SearchResult,
    scope: Option<&std::path::Path>,
) -> String {
    let path = std::path::Path::new(&result.path);
    let parent = path.parent().unwrap_or(std::path::Path::new(""));

    let label = if let Some(scope) = scope {
        parent
            .strip_prefix(scope)
            .unwrap_or(parent)
            .display()
            .to_string()
    } else {
        parent.display().to_string()
    };

    if label.is_empty() || label == "." {
        "dir: ./".to_string()
    } else {
        format!("dir: {}/", label.trim_end_matches('/'))
    }
}

fn extension_group_label(result: &vicaya_index::SearchResult) -> String {
    let path = std::path::Path::new(&result.path);
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) if !ext.is_empty() => format!("ext: {}", ext.to_lowercase()),
        _ => "ext: (none)".to_string(),
    }
}

/// Truncate path intelligently for display.
fn truncate_path(path: &str, max_len: usize, show_full: bool) -> String {
    if show_full || path.len() <= max_len {
        return path.to_string();
    }

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
