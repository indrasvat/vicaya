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

pub fn render(f: &mut Frame, area: Rect, app: &mut AppState) {
    let results = &app.search.results;
    let selected = app.search.selected_index;

    // Update scrolling state.
    let viewport_height = area.height.saturating_sub(2) as usize; // borders
    app.ui.viewport_height = viewport_height.max(1);
    app.ui.update_scroll(selected);

    let start = app.ui.scroll_offset.min(results.len());
    let end = (start + viewport_height).min(results.len());

    // Calculate available width for path display (rough estimate)
    let available_width = area.width.saturating_sub(4); // Account for borders
    let max_path_len = available_width.saturating_sub(30) as usize; // Reserve space for name, score, marker

    let items: Vec<ListItem> = results[start..end]
        .iter()
        .enumerate()
        .map(|(i, result)| {
            let absolute_index = start + i;
            let marker = if absolute_index == selected {
                "▸"
            } else {
                " "
            };
            let score_color = ui::score_color(result.score);
            let is_selected = absolute_index == selected;

            let path = std::path::Path::new(&result.path);
            let dir_path = path.parent().and_then(|p| p.to_str()).unwrap_or("");

            // Truncate path if not selected
            let display_path = truncate_path(dir_path, max_path_len.max(30), is_selected);

            let mut spans = vec![
                Span::styled(marker, Style::default().fg(ui::PRIMARY)),
                Span::raw(" "),
            ];

            if app.view == crate::state::ViewKind::Sthana {
                spans.push(Span::styled("📁 ", Style::default().fg(ui::ACCENT)));
            }

            spans.extend(vec![
                Span::styled(&result.name, Style::default().fg(ui::TEXT_PRIMARY)),
                Span::raw(" "),
                Span::styled(
                    format!("({}) ", display_path),
                    Style::default()
                        .fg(ui::TEXT_MUTED)
                        .add_modifier(Modifier::DIM),
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
        })
        .collect();

    let border_style = if app.search.is_results_focused() {
        Style::default().fg(ui::BORDER_FOCUS)
    } else {
        Style::default().fg(ui::BORDER_DIM)
    };

    let title = if app.search.is_searching {
        format!("phala ({})  searching…", results.len())
    } else {
        format!("phala ({})", results.len())
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
