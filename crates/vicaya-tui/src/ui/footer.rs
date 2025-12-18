//! Footer / status bar rendering.

use crate::state::AppState;
use crate::ui;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

pub fn render(f: &mut Frame, area: Rect, app: &AppState) {
    let mut spans = vec![
        Span::styled("Tab:", Style::default().fg(ui::PRIMARY)),
        Span::styled(" focus  ", Style::default().fg(ui::TEXT_SECONDARY)),
        Span::styled("Ctrl+T:", Style::default().fg(ui::PRIMARY)),
        Span::styled(" drishti  ", Style::default().fg(ui::TEXT_SECONDARY)),
        Span::styled("Ctrl+O:", Style::default().fg(ui::PRIMARY)),
        Span::styled(" purvadarshana  ", Style::default().fg(ui::TEXT_SECONDARY)),
        Span::styled("?:", Style::default().fg(ui::PRIMARY)),
        Span::styled(" help  ", Style::default().fg(ui::TEXT_SECONDARY)),
    ];

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
        Span::styled("Ctrl-C:", Style::default().fg(ui::PRIMARY)),
        Span::styled(" quit", Style::default().fg(ui::TEXT_SECONDARY)),
    ]);

    if let Some(msg) = &app.error {
        spans.push(Span::styled("  ", Style::default()));

        let color = if msg.starts_with('✓') {
            ui::SUCCESS
        } else {
            ui::ERROR
        };
        spans.push(Span::styled(
            msg,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));
    }

    let hints = Paragraph::new(Line::from(spans)).style(Style::default().bg(ui::BG_SURFACE));
    f.render_widget(hints, area);
}
