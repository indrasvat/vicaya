//! Preview pane rendering.

use crate::state::AppState;
use crate::ui;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

pub fn render(f: &mut Frame, area: Rect, app: &AppState) {
    let title = if app.preview.title.is_empty() {
        "Purvadarshana".to_string()
    } else {
        format!("Purvadarshana — {}", app.preview.title)
    };

    let border_style = Style::default().fg(ui::BORDER_DIM);

    let (text, content_style) = if !app.preview.is_visible {
        (vec![Line::raw("")], Style::default().fg(ui::TEXT_MUTED))
    } else if app.preview.is_loading {
        (
            vec![Line::raw("Loading preview…")],
            Style::default()
                .fg(ui::TEXT_SECONDARY)
                .add_modifier(Modifier::ITALIC),
        )
    } else if app.preview.lines.is_empty() {
        (
            vec![
                Line::raw("Select a result to preview its contents."),
                Line::raw(""),
                Line::raw("Ctrl+O toggles Purvadarshana."),
            ],
            Style::default().fg(ui::TEXT_MUTED),
        )
    } else {
        // Render only the visible slice to avoid rebuilding large previews every frame.
        let viewport_height = area.height.saturating_sub(2) as usize; // borders
        let start = app.preview.scroll as usize;
        let end = (start + viewport_height).min(app.preview.lines.len());
        let mut lines = Vec::with_capacity(end.saturating_sub(start));
        for s in &app.preview.lines[start..end] {
            lines.push(Line::raw(s.as_str()));
        }
        (lines, Style::default().fg(ui::TEXT_PRIMARY))
    };

    let preview = Paragraph::new(text)
        .style(content_style)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(title),
        )
        .wrap(Wrap { trim: false })
        .scroll((0, 0));

    f.render_widget(preview, area);
}
