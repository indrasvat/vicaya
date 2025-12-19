//! Footer / status bar rendering.

use crate::state::AppState;
use crate::ui;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use unicode_width::UnicodeWidthStr;
use vicaya_core::build_info::BUILD_INFO;

pub fn render(f: &mut Frame, area: Rect, app: &AppState) {
    let mut spans = vec![
        Span::styled("Tab:", Style::default().fg(ui::PRIMARY)),
        Span::styled(" focus  ", Style::default().fg(ui::TEXT_SECONDARY)),
        Span::styled("Ctrl+T:", Style::default().fg(ui::PRIMARY)),
        Span::styled(" drishti  ", Style::default().fg(ui::TEXT_SECONDARY)),
        Span::styled("Ctrl+P:", Style::default().fg(ui::PRIMARY)),
        Span::styled(" kriya-suchi  ", Style::default().fg(ui::TEXT_SECONDARY)),
        Span::styled("Ctrl+O:", Style::default().fg(ui::PRIMARY)),
        Span::styled(" purvadarshana  ", Style::default().fg(ui::TEXT_SECONDARY)),
        Span::styled("Ctrl+G:", Style::default().fg(ui::PRIMARY)),
        Span::styled(" varga  ", Style::default().fg(ui::TEXT_SECONDARY)),
        Span::styled("?:", Style::default().fg(ui::PRIMARY)),
        Span::styled(" help  ", Style::default().fg(ui::TEXT_SECONDARY)),
    ];

    if app.search.is_results_focused() {
        spans.extend(vec![
            Span::styled("↵:", Style::default().fg(ui::PRIMARY)),
            Span::styled(" open/enter  ", Style::default().fg(ui::TEXT_SECONDARY)),
            Span::styled("h/l:", Style::default().fg(ui::PRIMARY)),
            Span::styled(" ksetra  ", Style::default().fg(ui::TEXT_SECONDARY)),
            Span::styled("y:", Style::default().fg(ui::PRIMARY)),
            Span::styled(" copy  ", Style::default().fg(ui::TEXT_SECONDARY)),
            Span::styled("p:", Style::default().fg(ui::PRIMARY)),
            Span::styled(" print  ", Style::default().fg(ui::TEXT_SECONDARY)),
            Span::styled("r:", Style::default().fg(ui::PRIMARY)),
            Span::styled(" reveal  ", Style::default().fg(ui::TEXT_SECONDARY)),
        ]);
    }

    if app.search.is_preview_focused() || app.mode == crate::state::AppMode::PreviewSearch {
        spans.extend(vec![
            Span::styled("/:", Style::default().fg(ui::PRIMARY)),
            Span::styled(" find  ", Style::default().fg(ui::TEXT_SECONDARY)),
            Span::styled("n/N:", Style::default().fg(ui::PRIMARY)),
            Span::styled(" next/prev  ", Style::default().fg(ui::TEXT_SECONDARY)),
            Span::styled("Ctrl+N:", Style::default().fg(ui::PRIMARY)),
            Span::styled(" lines  ", Style::default().fg(ui::TEXT_SECONDARY)),
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

    let build_info = format!(" {}", compact_build_info(app));
    let build_width = (UnicodeWidthStr::width(build_info.as_str()) as u16).min(area.width);

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(build_width)])
        .split(area);

    let hints = Paragraph::new(Line::from(spans)).style(Style::default().bg(ui::BG_SURFACE));
    let build = Paragraph::new(build_info)
        .style(
            Style::default()
                .fg(ui::TEXT_SECONDARY)
                .bg(ui::BG_SURFACE)
                .add_modifier(Modifier::ITALIC),
        )
        .alignment(Alignment::Right);

    f.render_widget(hints, chunks[0]);
    f.render_widget(build, chunks[1]);
}

fn compact_build_info(app: &AppState) -> String {
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

    if let Some(status) = &app.daemon_status {
        let daemon_sha = status.build.git_sha.as_str();
        if !daemon_sha.is_empty() && daemon_sha != "unknown" {
            out.push(' ');
            out.push('d');
            out.push('@');
            for (idx, ch) in daemon_sha.chars().enumerate() {
                if idx >= 7 {
                    break;
                }
                out.push(ch);
            }

            if sha != "unknown" && daemon_sha != sha {
                out.push('!');
            }
        }
    }

    out
}
