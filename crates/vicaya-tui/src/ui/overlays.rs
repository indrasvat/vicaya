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
        "vicaya-tui — drishti / ksetra quick help",
        "",
        "Core terms:",
        "  drishti       View mode (Files, Dirs, …)",
        "  prashna       Query input",
        "  phala         Results list",
        "  purvadarshana Preview pane",
        "",
        "Keys:",
        "  Tab           Cycle focus (prashna / phala / purvadarshana)",
        "  Shift+Tab     Cycle focus (reverse)",
        "  Ctrl+T        drishti switcher",
        "  Ctrl+O        Toggle purvadarshana",
        "  ↓ (in input)  Move to phala",
        "  ↑ (at top)    Move to prashna",
        "",
        "Navigation (phala):",
        "  j / ↓         Down",
        "  k / ↑         Up",
        "  g / G         Top / Bottom",
        "",
        "Preview (purvadarshana):",
        "  PgUp / PgDn   Scroll preview",
        "  Ctrl+U / Ctrl+D  Scroll preview",
        "",
        "Actions (phala):",
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
                .title(" drishti ")
                .style(Style::default().bg(ui::BG_DARK)),
        )
        .style(Style::default().bg(ui::BG_DARK))
        .highlight_style(Style::default().bg(ui::BG_ELEVATED).fg(ui::PRIMARY))
        .highlight_symbol("▸ ");

    let mut state = ListState::default();
    state.select(Some(app.ui.drishti_switcher.selected_index));
    f.render_stateful_widget(list, area, &mut state);
}
