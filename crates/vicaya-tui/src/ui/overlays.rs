//! Overlay rendering (help, dialogs, switchers).

use crate::state::AppState;
use crate::ui;
use ratatui::{
    layout::Rect,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

pub fn render_help(f: &mut Frame) {
    let help_text = vec![
        "vicaya-tui — drishti / ksetra quick help",
        "",
        "Core terms:",
        "  drishti       View mode (Files, Dirs, …)",
        "  prashna       Query input",
        "  niyama        Filters (chips in prashna)",
        "  phala         Results list",
        "  purvadarshana Preview pane",
        "",
        "Keys:",
        "  Tab           Cycle focus (prashna / phala / purvadarshana)",
        "  Shift+Tab     Cycle focus (reverse)",
        "  Ctrl+T        drishti switcher (type to filter)",
        "  Ctrl+O        Toggle purvadarshana",
        "  Ctrl+G        Cycle varga grouping (none/dir/ext)",
        "  ↓ (in input)  Move to phala",
        "  ↑ (at top)    Move to prashna",
        "",
        "Navigation (phala):",
        "  j / ↓         Down",
        "  k / ↑         Up",
        "  g / G         Top / Bottom",
        "  h / l         Ksetra pop / push (dirs)",
        "",
        "Preview (purvadarshana):",
        "  PgUp / PgDn   Scroll preview",
        "  Ctrl+U / Ctrl+D  Scroll preview",
        "  /             Search in preview",
        "  n / N         Next / previous match",
        "  Ctrl+N        Toggle line numbers",
        "",
        "Actions (phala):",
        "  Enter / o     Open (files) / Enter scope (dirs)",
        "  y             Copy path",
        "  p             Print path and exit",
        "  r             Reveal in file manager",
        "",
        "Niyama syntax:",
        "  ext:rs,md  type:file|dir  path:src/  size:>10mb  mtime:<7d",
        "",
        "Press Esc to close",
    ];

    let help = Paragraph::new(help_text.join("\n"))
        .style(Style::default().fg(ui::TEXT_PRIMARY).bg(ui::BG_DARK))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(ui::PRIMARY))
                .title(" Help ")
                .style(Style::default().bg(ui::BG_DARK)),
        );

    let area = crate::ui::layout::centered_rect(70, 80, f.area());
    f.render_widget(Clear, area);
    f.render_widget(help, area);
}

pub fn render_confirm(f: &mut Frame, _app: &AppState) {
    let confirm = Paragraph::new("Are you sure? (y/n)")
        .style(Style::default().fg(ui::TEXT_PRIMARY).bg(ui::BG_DARK))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(ui::WARNING)),
        );

    let area = crate::ui::layout::centered_rect(40, 20, f.area());
    f.render_widget(Clear, area);
    f.render_widget(confirm, area);
}

pub fn render_drishti_switcher(f: &mut Frame, app: &AppState) {
    use ratatui::widgets::ListState;

    let area = crate::ui::layout::centered_rect(60, 65, f.area());
    f.render_widget(Clear, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let filter = app.ui.drishti_switcher.filter_query();
    let filter_input = Paragraph::new(Line::from(vec![
        Span::styled("filter: ", Style::default().fg(ui::ACCENT)),
        Span::styled(filter, Style::default().fg(ui::TEXT_PRIMARY)),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(ui::PRIMARY))
            .title(" drishti "),
    )
    .style(Style::default().bg(ui::BG_DARK));

    f.render_widget(filter_input, chunks[0]);
    let cursor_x = chunks[0].x + 1 + "filter: ".len() as u16 + filter.len() as u16;
    let cursor_y = chunks[0].y + 1;
    f.set_cursor_position((cursor_x, cursor_y));

    let views = app.ui.drishti_switcher.matching_views();
    let items: Vec<ListItem> = if views.is_empty() {
        vec![ListItem::new(Line::from(vec![Span::styled(
            " (no matches)",
            Style::default()
                .fg(ui::TEXT_MUTED)
                .add_modifier(Modifier::ITALIC),
        )]))]
    } else {
        views
            .iter()
            .map(|view| {
                let enabled = view.is_enabled();
                let prefix = if enabled { " " } else { "·" };
                let label = format!("{prefix} {:<12}  {}", view.label(), view.english_hint());

                let style = if enabled {
                    Style::default().fg(ui::TEXT_PRIMARY)
                } else {
                    Style::default()
                        .fg(ui::TEXT_MUTED)
                        .add_modifier(Modifier::DIM)
                };

                ListItem::new(Line::from(vec![Span::styled(label, style)]))
            })
            .collect()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(ui::PRIMARY))
                .title(" choose ")
                .style(Style::default().bg(ui::BG_DARK)),
        )
        .style(Style::default().bg(ui::BG_DARK))
        .highlight_style(Style::default().bg(ui::BG_ELEVATED).fg(ui::PRIMARY))
        .highlight_symbol("▸ ");

    let mut state = ListState::default();
    if views.is_empty() {
        state.select(None);
    } else {
        state.select(Some(
            app.ui
                .drishti_switcher
                .selected_index
                .min(views.len().saturating_sub(1)),
        ));
    }
    f.render_stateful_widget(list, chunks[1], &mut state);
}

pub fn render_preview_search(f: &mut Frame, app: &AppState) {
    let root = f.area();
    let width = ((root.width as f32) * 0.72) as u16;
    let width = width.clamp(40, root.width.saturating_sub(2));
    let height = 5u16;
    let area = centered_fixed_rect(width, height, root);

    f.render_widget(Clear, area);

    let query = app.preview.search_input.as_str();
    let input = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("purvadarshana /: ", Style::default().fg(ui::ACCENT)),
            Span::styled(query, Style::default().fg(ui::TEXT_PRIMARY)),
        ]),
        Line::from(vec![Span::styled(
            "Enter: apply   Esc: cancel",
            Style::default()
                .fg(ui::TEXT_SECONDARY)
                .add_modifier(Modifier::ITALIC),
        )]),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(ui::PRIMARY))
            .title(" preview search ")
            .style(Style::default().bg(ui::BG_DARK)),
    )
    .style(Style::default().bg(ui::BG_DARK));

    f.render_widget(input, area);

    let cursor_x = area.x + 1 + "purvadarshana /: ".len() as u16 + app.preview.search_cursor as u16;
    let cursor_y = area.y + 1;
    f.set_cursor_position((cursor_x, cursor_y));
}

fn centered_fixed_rect(width: u16, height: u16, r: Rect) -> Rect {
    let width = width.min(r.width);
    let height = height.min(r.height);

    let x = r.x + (r.width.saturating_sub(width)) / 2;
    let y = r.y + (r.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width,
        height,
    }
}
