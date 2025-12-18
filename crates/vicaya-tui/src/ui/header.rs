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
        "drishti: {} ({})",
        app.view.label(),
        app.view.english_hint()
    );
    let ksetra = "ksetra: global";

    let (rakshaka_text, rakshaka_color, suchi_text, reconciling) =
        if let Some(status) = &app.daemon_status {
            let suchi = format!("suchi  {}", format_count(status.indexed_files));
            let reconciling = status.reconciling;
            let rakshaka = "rakshaka  ok";
            let rakshaka_color = if reconciling {
                ui::WARNING
            } else {
                ui::SUCCESS
            };
            (rakshaka.to_string(), rakshaka_color, suchi, reconciling)
        } else {
            (
                "rakshaka  offline".to_string(),
                ui::ERROR,
                "suchi  ?".to_string(),
                false,
            )
        };

    let sep = Span::styled(" | ", Style::default().fg(ui::TEXT_MUTED));

    let mut spans = vec![
        Span::styled(
            "vicaya",
            Style::default()
                .fg(ui::PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        sep.clone(),
        Span::styled("◎ ", Style::default().fg(ui::ACCENT)),
        Span::styled(drishti, Style::default().fg(ui::ACCENT)),
        sep.clone(),
        Span::styled("⌁ ", Style::default().fg(ui::TEXT_SECONDARY)),
        Span::styled(ksetra, Style::default().fg(ui::TEXT_SECONDARY)),
        sep.clone(),
        Span::styled("● ", Style::default().fg(rakshaka_color)),
        Span::styled(rakshaka_text, Style::default().fg(rakshaka_color)),
        sep.clone(),
        Span::styled("≡ ", Style::default().fg(ui::INFO)),
        Span::styled(suchi_text, Style::default().fg(ui::INFO)),
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

fn format_count(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (count, ch) in s.chars().rev().enumerate() {
        if count != 0 && count.is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}
