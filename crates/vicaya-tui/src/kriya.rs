//! Kriya (actions) and Kriya-Suchi (action palette) helpers.

use crate::state::{AppState, ViewKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KriyaId {
    OpenOrEnter,
    CopyPath,
    Reveal,
    PrintPath,
    TogglePreview,
    ToggleGrouping,
    PopKsetra,
    SetKsetra,
    TogglePreviewLineNumbers,
    ClearPreviewSearch,
    Quit,
}

#[derive(Debug, Clone)]
pub struct KriyaItem {
    pub id: KriyaId,
    pub label: &'static str,
    pub keys: &'static str,
    pub hint: &'static str,
    pub destructive: bool,
}

pub fn filtered_kriyas(app: &AppState) -> Vec<KriyaItem> {
    let items = available_kriyas(app);
    let filter = app.ui.kriya_suchi.filter_query().trim().to_lowercase();
    if filter.is_empty() {
        return items;
    }

    items
        .into_iter()
        .filter(|item| matches_filter(item, &filter))
        .collect()
}

fn matches_filter(item: &KriyaItem, filter: &str) -> bool {
    item.label.to_lowercase().contains(filter)
        || item.hint.to_lowercase().contains(filter)
        || item.keys.to_lowercase().contains(filter)
}

fn available_kriyas(app: &AppState) -> Vec<KriyaItem> {
    let selected = app.search.selected_result();
    let selected_path = selected.map(|r| r.path.as_str());
    let selected_is_dir = selected_path.map(|p| is_dir_for_view(p, app.view));

    let mut items = Vec::new();

    if let (Some(_), Some(is_dir)) = (selected_path, selected_is_dir) {
        items.push(KriyaItem {
            id: KriyaId::OpenOrEnter,
            label: if is_dir {
                "Enter ksetra"
            } else {
                "Open in editor"
            },
            keys: if is_dir { "Enter/o, l/→" } else { "Enter/o" },
            hint: if is_dir {
                "Push scope to directory"
            } else {
                "Open in $EDITOR"
            },
            destructive: false,
        });

        items.extend([
            KriyaItem {
                id: KriyaId::CopyPath,
                label: "Copy path",
                keys: "y",
                hint: "Copy absolute path to clipboard",
                destructive: false,
            },
            KriyaItem {
                id: KriyaId::Reveal,
                label: "Reveal",
                keys: "r",
                hint: "Reveal in Finder / file manager",
                destructive: false,
            },
            KriyaItem {
                id: KriyaId::PrintPath,
                label: "Print path",
                keys: "p",
                hint: "Print path and exit (shell integration)",
                destructive: false,
            },
        ]);
    }

    if app.ksetra.depth() > 0 {
        items.push(KriyaItem {
            id: KriyaId::PopKsetra,
            label: "Pop ksetra",
            keys: "h/←",
            hint: "Go back toward global scope",
            destructive: false,
        });
    }

    // Always available: direct ksetra input
    items.push(KriyaItem {
        id: KriyaId::SetKsetra,
        label: "Set ksetra",
        keys: "Ctrl+K",
        hint: "Type path directly to set scope",
        destructive: false,
    });

    items.extend([
        KriyaItem {
            id: KriyaId::TogglePreview,
            label: "Toggle purvadarshana",
            keys: "Ctrl+O",
            hint: "Show/hide preview pane",
            destructive: false,
        },
        KriyaItem {
            id: KriyaId::ToggleGrouping,
            label: "Cycle varga",
            keys: "Ctrl+G",
            hint: "Toggle grouping (none/dir/ext)",
            destructive: false,
        },
    ]);

    if app.preview.is_visible && !app.preview.lines.is_empty() {
        items.push(KriyaItem {
            id: KriyaId::TogglePreviewLineNumbers,
            label: "Toggle line numbers",
            keys: "Ctrl+N",
            hint: "Show/hide preview line numbers",
            destructive: false,
        });
    }

    if !app.preview.search_query.trim().is_empty() {
        items.push(KriyaItem {
            id: KriyaId::ClearPreviewSearch,
            label: "Clear preview search",
            keys: "Ctrl+L",
            hint: "Remove /.../ highlight and matches",
            destructive: false,
        });
    }

    items.push(KriyaItem {
        id: KriyaId::Quit,
        label: "Quit",
        keys: "Ctrl+C, q",
        hint: "Exit vicaya-tui",
        destructive: true,
    });

    items
}

fn is_dir_for_view(path: &str, view: ViewKind) -> bool {
    if view == ViewKind::Sthana {
        return true;
    }

    std::fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false)
}
