//! Preview pane rendering.

use crate::state::{AppMode, AppState, TextKind};
use crate::ui;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub fn render(f: &mut Frame, area: Rect, app: &AppState) {
    let search_query = if app.mode == AppMode::PreviewSearch {
        app.preview.search_input.trim()
    } else {
        app.preview.search_query.trim()
    };

    let title = if app.preview.title.is_empty() {
        "purvadarshana".to_string()
    } else {
        let search = if search_query.is_empty() {
            String::new()
        } else {
            format!(" /{search_query}/")
        };
        let truncated = if app.preview.truncated {
            "  (truncated)"
        } else {
            ""
        };
        format!("purvadarshana — {}{search}{truncated}", app.preview.title)
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

        for (i, line) in app.preview.lines[start..end].iter().enumerate() {
            let line_index = start + i;

            let mut spans = Vec::new();
            if app.preview.show_line_numbers {
                let num = app
                    .preview
                    .content_line_numbers
                    .get(line_index)
                    .copied()
                    .flatten();
                let prefix = if let Some(n) = num {
                    format!("{:>4} ", n)
                } else {
                    "     ".to_string()
                };
                spans.push(Span::styled(
                    prefix,
                    Style::default().fg(ui::TEXT_SECONDARY),
                ));
            }

            spans.extend(line_spans(line, search_query));
            if spans.is_empty() {
                lines.push(Line::raw(""));
            } else {
                lines.push(Line::from(spans));
            }
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

fn line_spans(line: &crate::state::StyledLine, query: &str) -> Vec<Span<'static>> {
    let query = query.trim();
    if query.is_empty() {
        return line
            .iter()
            .map(|seg| Span::styled(seg.text.clone(), segment_style(seg.style)))
            .collect();
    }

    let mut full = String::new();
    for seg in line {
        full.push_str(seg.text.as_str());
    }

    let matches = find_matches(&full, query);
    if matches.is_empty() {
        return line
            .iter()
            .map(|seg| Span::styled(seg.text.clone(), segment_style(seg.style)))
            .collect();
    }

    // Rebuild spans with match highlights while preserving per-segment styles.
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut offset = 0usize;

    for seg in line {
        let seg_text = seg.text.as_str();
        if seg_text.is_empty() {
            continue;
        }

        let seg_start = offset;
        let seg_end = seg_start + seg_text.len();
        offset = seg_end;

        let base_style = segment_style(seg.style);
        let mut cursor = seg_start;
        for (m_start, m_end) in &matches {
            if *m_end <= seg_start {
                continue;
            }
            if *m_start >= seg_end {
                break;
            }

            let hi_start = (*m_start).max(seg_start);
            let hi_end = (*m_end).min(seg_end);
            if cursor < hi_start {
                let rel_start = cursor - seg_start;
                let rel_end = hi_start - seg_start;
                spans.push(Span::styled(
                    seg_text[rel_start..rel_end].to_string(),
                    base_style,
                ));
            }

            let rel_start = hi_start - seg_start;
            let rel_end = hi_end - seg_start;
            spans.push(Span::styled(
                seg_text[rel_start..rel_end].to_string(),
                base_style.bg(ui::PRIMARY_DIM).add_modifier(Modifier::BOLD),
            ));
            cursor = hi_end;
        }

        if cursor < seg_end {
            let rel_start = cursor - seg_start;
            spans.push(Span::styled(seg_text[rel_start..].to_string(), base_style));
        }
    }

    spans
}

fn find_matches(haystack: &str, needle: &str) -> Vec<(usize, usize)> {
    if needle.is_empty() {
        return Vec::new();
    }

    let (hay, needle) = if haystack.is_ascii() && needle.is_ascii() {
        (haystack.to_ascii_lowercase(), needle.to_ascii_lowercase())
    } else {
        (haystack.to_string(), needle.to_string())
    };

    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(pos) = hay[from..].find(&needle) {
        let start = from + pos;
        let end = start + needle.len();
        out.push((start, end));
        from = end;
        if from >= hay.len() {
            break;
        }
    }

    out
}
