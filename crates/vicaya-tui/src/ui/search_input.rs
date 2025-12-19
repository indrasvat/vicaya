//! Search input rendering.

use crate::state::AppState;
use crate::ui;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub fn render(f: &mut Frame, area: Rect, app: &AppState) {
    let query = &app.search.query;
    let cursor_pos = app.search.cursor_position;
    let is_focused = app.search.is_input_focused();

    let parsed = crate::state::parse_query(query);

    let border_style = if is_focused {
        Style::default().fg(ui::BORDER_FOCUS)
    } else {
        Style::default().fg(ui::BORDER_DIM)
    };

    let mut lines = Vec::with_capacity(2);
    lines.push(Line::from(vec![
        Span::styled("prashna: ", Style::default().fg(ui::ACCENT)),
        Span::styled(query, Style::default().fg(ui::TEXT_PRIMARY)),
    ]));

    if parsed.niyamas.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("niyama: ", Style::default().fg(ui::TEXT_SECONDARY)),
            Span::styled("—", Style::default().fg(ui::TEXT_MUTED)),
        ]));
    } else {
        let mut spans = vec![Span::styled(
            "niyama: ",
            Style::default().fg(ui::TEXT_SECONDARY),
        )];

        for (idx, niyama) in parsed.niyamas.iter().enumerate() {
            if idx > 0 {
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(
                niyama.raw(),
                Style::default()
                    .fg(ui::TEXT_PRIMARY)
                    .bg(ui::BG_ELEVATED)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        lines.push(Line::from(spans));
    }

    let input = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .style(if is_focused {
                Style::default().bg(ui::BG_ELEVATED)
            } else {
                Style::default().bg(ui::BG_SURFACE)
            }),
    );

    f.render_widget(input, area);

    if is_focused {
        // Cursor position: 1 (border) + len("Prashna: ") + cursor
        let cursor_x = area.x + 1 + "prashna: ".len() as u16 + cursor_pos as u16;
        let cursor_y = area.y + 1;
        f.set_cursor_position((cursor_x, cursor_y));
    }
}
