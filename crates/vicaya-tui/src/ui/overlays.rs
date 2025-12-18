//! Overlay rendering (help, dialogs, switchers).

use crate::state::{AppState, ViewKind};
use crate::ui;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

pub fn render_help(f: &mut Frame) {
    let help_text = vec![
        "vicaya-tui — Drishti / Ksetra quick help",
        "",
        "Core terms:",
        "  Drishti       View mode (Files, Dirs, …)",
        "  Prashna       Query input",
        "  Phala         Results list",
        "  Purvadarshana Preview pane",
        "",
        "Keys:",
        "  Tab           Toggle focus (Prashna / Phala)",
        "  Ctrl+T        Drishti switcher",
        "  Ctrl+O        Toggle Purvadarshana",
        "  ↓ (in input)  Move to Phala",
        "  ↑ (at top)    Move to Prashna",
        "",
        "Navigation (Phala):",
        "  j / ↓         Down",
        "  k / ↑         Up",
        "  g / G         Top / Bottom",
        "",
        "Actions (Phala):",
        "  Enter / o     Open in $EDITOR",
        "  y             Copy path",
        "  p             Print path and exit",
        "  r             Reveal in file manager",
        "",
        "Press Esc to close",
    ];

    let help = Paragraph::new(help_text.join("\n"))
        .style(Style::default().fg(ui::TEXT_PRIMARY).bg(ui::BG_DARK))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(ui::PRIMARY))
                .title(" Help "),
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

    let items: Vec<ListItem> = ViewKind::ALL
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
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(ui::PRIMARY))
                .title(" Drishti "),
        )
        .highlight_style(Style::default().bg(ui::BG_ELEVATED).fg(ui::PRIMARY))
        .highlight_symbol("▸ ");

    let mut state = ListState::default();
    state.select(Some(app.ui.drishti_switcher.selected_index));
    f.render_stateful_widget(list, area, &mut state);
}
