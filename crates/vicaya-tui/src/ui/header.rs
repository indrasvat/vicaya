//! Header rendering.

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
    let drishti = format!(
        "Drishti: {} ({})",
        app.view.label(),
        app.view.english_hint()
    );
    let ksetra = "Ksetra: Global";

    let (rakshaka, suchi, reconciling) = if let Some(status) = &app.daemon_status {
        let rakshaka = "Rakshaka: OK".to_string();
        let suchi = format!("Suchi: {}", status.indexed_files);
        (rakshaka, suchi, status.reconciling)
    } else {
        (
            "Rakshaka: Offline".to_string(),
            "Suchi: ?".to_string(),
            false,
        )
    };

    let mut spans = vec![
        Span::styled(
            "vicaya",
            Style::default()
                .fg(ui::PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ", Style::default()),
        Span::styled(drishti, Style::default().fg(ui::ACCENT)),
        Span::styled("  ", Style::default()),
        Span::styled(ksetra, Style::default().fg(ui::TEXT_SECONDARY)),
        Span::styled("  ", Style::default()),
        Span::styled(
            rakshaka,
            Style::default().fg(if app.daemon_status.is_some() {
                ui::SUCCESS
            } else {
                ui::WARNING
            }),
        ),
        Span::styled("  ", Style::default()),
        Span::styled(suchi, Style::default().fg(ui::INFO)),
    ];

    if reconciling {
        spans.push(Span::styled("  ", Style::default()));
        spans.push(Span::styled(
            "reconciling…",
            Style::default()
                .fg(ui::WARNING)
                .add_modifier(Modifier::ITALIC),
        ));
    }

    let header = Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(ui::BORDER_DIM))
            .style(Style::default().bg(ui::BG_SURFACE)),
    );

    f.render_widget(header, area);
}
