//! Preview pane rendering.

use crate::state::{AppState, TextKind};
use crate::ui;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub fn render(f: &mut Frame, area: Rect, app: &AppState) {
    let title = if app.preview.title.is_empty() {
        "purvadarshana".to_string()
    } else {
        let truncated = if app.preview.truncated {
            "  (truncated)"
        } else {
            ""
        };
        format!("purvadarshana — {}{truncated}", app.preview.title)
    };

    let border_style = if app.search.is_preview_focused() {
        Style::default().fg(ui::BORDER_FOCUS)
    } else {
        Style::default().fg(ui::BORDER_DIM)
    };

    let text = if !app.preview.is_visible {
        vec![Line::raw("")]
    } else if app.preview.is_loading {
        vec![Line::styled(
            "loading preview…",
            Style::default()
                .fg(ui::TEXT_SECONDARY)
                .add_modifier(Modifier::ITALIC),
        )]
    } else if app.preview.lines.is_empty() {
        vec![
            Line::styled(
                "Select a result to preview its contents.",
                Style::default().fg(ui::TEXT_MUTED),
            ),
            Line::raw(""),
            Line::styled(
                "Ctrl+O toggles purvadarshana.",
                Style::default().fg(ui::TEXT_MUTED),
            ),
        ]
    } else {
        let viewport_height = area.height.saturating_sub(2) as usize; // borders
        let start = app.preview.scroll as usize;
        let end = (start + viewport_height).min(app.preview.lines.len());
        let mut lines = Vec::with_capacity(end.saturating_sub(start));

        for line in &app.preview.lines[start..end] {
            if line.is_empty() {
                lines.push(Line::raw(""));
                continue;
            }

            let spans: Vec<Span> = line
                .iter()
                .map(|seg| Span::styled(seg.text.as_str(), segment_style(seg.style)))
                .collect();
            lines.push(Line::from(spans));
        }

        lines
    };

    let preview = Paragraph::new(text)
        .style(Style::default().bg(ui::BG_SURFACE))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(title)
                .style(Style::default().bg(ui::BG_SURFACE)),
        );

    f.render_widget(preview, area);
}

fn segment_style(style: crate::state::TextStyle) -> Style {
    let mut out = match style.kind {
        TextKind::Normal => Style::default().fg(ui::TEXT_PRIMARY),
        TextKind::Meta => Style::default().fg(ui::TEXT_MUTED),
        TextKind::Error => Style::default().fg(ui::ERROR).add_modifier(Modifier::BOLD),
    };

    if let Some((r, g, b)) = style.fg {
        out = out.fg(Color::Rgb(r, g, b));
    }
    if style.bold {
        out = out.add_modifier(Modifier::BOLD);
    }
    if style.italic {
        out = out.add_modifier(Modifier::ITALIC);
    }
    if style.underline {
        out = out.add_modifier(Modifier::UNDERLINED);
    }
    out
}
