//! Header rendering.

use crate::state::AppState;
use crate::ui;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use vicaya_core::build_info::BUILD_INFO;

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

    let build_info = compact_build_info();
    let build_width = (build_info.len() as u16).min(area.width.saturating_sub(2));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ui::BORDER_DIM))
        .style(Style::default().bg(ui::BG_SURFACE));
    let inner = block.inner(area);

    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(build_width)])
        .split(inner);

    let left = Paragraph::new(Line::from(spans)).style(Style::default().bg(ui::BG_SURFACE));
    let right = Paragraph::new(build_info)
        .style(
            Style::default()
                .fg(ui::TEXT_MUTED)
                .bg(ui::BG_SURFACE)
                .add_modifier(Modifier::DIM),
        )
        .alignment(Alignment::Right);

    f.render_widget(left, chunks[0]);
    f.render_widget(right, chunks[1]);
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

fn compact_build_info() -> String {
    let version = BUILD_INFO.version;
    let sha = BUILD_INFO.git_sha;

    let mut out = String::with_capacity(version.len() + sha.len().min(7) + 3);
    out.push('v');
    out.push_str(version);

    if sha != "unknown" {
        out.push('@');
        for (idx, ch) in sha.chars().enumerate() {
            if idx >= 7 {
                break;
            }
            out.push(ch);
        }
    }

    out
}
