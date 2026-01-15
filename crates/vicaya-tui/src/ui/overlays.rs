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
        "  Ctrl+P        kriya-suchi (action palette)",
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
        "  Ctrl+L        Clear preview search",
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

pub fn render_kriya_suchi(f: &mut Frame, app: &AppState) {
    use ratatui::widgets::ListState;

    let area = crate::ui::layout::centered_rect(72, 70, f.area());
    f.render_widget(Clear, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let filter = app.ui.kriya_suchi.filter_query();
    let filter_input = Paragraph::new(Line::from(vec![
        Span::styled("kriya: ", Style::default().fg(ui::ACCENT)),
        Span::styled(filter, Style::default().fg(ui::TEXT_PRIMARY)),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(ui::PRIMARY))
            .title(" kriya-suchi "),
    )
    .style(Style::default().bg(ui::BG_DARK));

    f.render_widget(filter_input, chunks[0]);
    let cursor_x = chunks[0].x + 1 + "kriya: ".len() as u16 + filter.len() as u16;
    let cursor_y = chunks[0].y + 1;
    f.set_cursor_position((cursor_x, cursor_y));

    let actions = crate::kriya::filtered_kriyas(app);
    let items: Vec<ListItem> = if actions.is_empty() {
        vec![ListItem::new(Line::from(vec![Span::styled(
            " (no matches)",
            Style::default()
                .fg(ui::TEXT_MUTED)
                .add_modifier(Modifier::ITALIC),
        )]))]
    } else {
        actions
            .iter()
            .map(|action| {
                let mut spans = Vec::new();

                spans.push(Span::styled(
                    format!("{:<20}", action.label),
                    Style::default()
                        .fg(if action.destructive {
                            ui::ERROR
                        } else {
                            ui::TEXT_PRIMARY
                        })
                        .add_modifier(Modifier::BOLD),
                ));

                if !action.keys.is_empty() {
                    spans.push(Span::styled(
                        format!("{:<12}", action.keys),
                        Style::default().fg(ui::PRIMARY),
                    ));
                } else {
                    spans.push(Span::raw("            "));
                }

                spans.push(Span::styled(
                    action.hint,
                    Style::default().fg(ui::TEXT_SECONDARY),
                ));

                ListItem::new(Line::from(spans))
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
    if actions.is_empty() {
        state.select(None);
    } else {
        state.select(Some(
            app.ui
                .kriya_suchi
                .selected_index
                .min(actions.len().saturating_sub(1)),
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

pub fn render_ksetra_input(f: &mut Frame, app: &AppState) {
    use crate::state::KsetraInputState;
    use ratatui::widgets::ListState;

    let root = f.area();
    let width = ((root.width as f32) * 0.75) as u16;
    let width = width.clamp(50, root.width.saturating_sub(4));

    // Height: input (3) + completions (up to 7) + help (2) + borders
    let completions_count = app.ksetra_input.completions.len().min(5);
    let height = if completions_count > 0 {
        3 + completions_count as u16 + 2 + 3
    } else {
        3 + 3
    };
    let area = centered_fixed_rect(width, height, root);

    f.render_widget(Clear, area);

    // Split into: input, completions (optional), help
    let constraints = if completions_count > 0 {
        vec![
            Constraint::Length(3),
            Constraint::Length(completions_count as u16 + 2),
            Constraint::Length(1),
        ]
    } else {
        vec![Constraint::Length(3), Constraint::Length(1)]
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    // Path input
    let input = &app.ksetra_input.input;
    let mut input_spans = vec![
        Span::styled("path: ", Style::default().fg(ui::ACCENT)),
        Span::styled(input.as_str(), Style::default().fg(ui::TEXT_PRIMARY)),
    ];

    // Show error if present
    if let Some(err) = &app.ksetra_input.error {
        input_spans.push(Span::styled(
            format!("  {}", err),
            Style::default()
                .fg(ui::ERROR)
                .add_modifier(Modifier::ITALIC),
        ));
    }

    let input_widget = Paragraph::new(Line::from(input_spans))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(ui::PRIMARY))
                .title(" ksetra ")
                .style(Style::default().bg(ui::BG_DARK)),
        )
        .style(Style::default().bg(ui::BG_DARK));

    f.render_widget(input_widget, chunks[0]);

    // Set cursor position
    let cursor_x = chunks[0].x + 1 + "path: ".len() as u16 + app.ksetra_input.cursor as u16;
    let cursor_y = chunks[0].y + 1;
    f.set_cursor_position((cursor_x, cursor_y));

    // Completions list (if any)
    if completions_count > 0 {
        let items: Vec<ListItem> = app
            .ksetra_input
            .completions
            .iter()
            .take(5)
            .map(|path| {
                let display = KsetraInputState::display_path(std::path::Path::new(path));
                ListItem::new(Line::from(vec![Span::styled(
                    display,
                    Style::default().fg(ui::TEXT_PRIMARY),
                )]))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(ui::BORDER_DIM))
                    .title(" completions ")
                    .style(Style::default().bg(ui::BG_DARK)),
            )
            .style(Style::default().bg(ui::BG_DARK))
            .highlight_style(Style::default().bg(ui::BG_ELEVATED).fg(ui::PRIMARY))
            .highlight_symbol("▸ ");

        let mut state = ListState::default();
        state.select(Some(
            app.ksetra_input
                .selected_completion
                .min(completions_count.saturating_sub(1)),
        ));
        f.render_stateful_widget(list, chunks[1], &mut state);
    }

    // Help text
    let help_chunk = if completions_count > 0 {
        chunks[2]
    } else {
        chunks[1]
    };
    let help = Paragraph::new(Line::from(vec![Span::styled(
        " Enter: set    Esc: cancel    Tab: complete    ~/: home",
        Style::default()
            .fg(ui::TEXT_SECONDARY)
            .add_modifier(Modifier::ITALIC),
    )]))
    .style(Style::default().bg(ui::BG_DARK));

    f.render_widget(help, help_chunk);
}
