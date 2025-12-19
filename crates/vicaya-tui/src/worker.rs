//! Background worker for daemon IPC and preview loading.

use crate::client::{DaemonStatus, IpcClient};
use crate::state::{Niyama, NiyamaType, StyledLine, StyledSegment, TextKind, TextStyle, ViewKind};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};
use vicaya_index::SearchResult;

use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle, Theme, ThemeSet},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};

pub enum WorkerCommand {
    Search {
        id: u64,
        query: String,
        limit: usize,
        view: ViewKind,
        scope: Option<std::path::PathBuf>,
        niyamas: Vec<Niyama>,
    },
    Preview {
        id: u64,
        path: String,
    },
    Quit,
}

pub enum WorkerEvent {
    SearchResults {
        id: u64,
        results: Vec<SearchResult>,
        error: Option<String>,
    },
    PreviewReady {
        id: u64,
        path: String,
        title: String,
        lines: Vec<StyledLine>,
        truncated: bool,
    },
    Status {
        status: Option<DaemonStatus>,
    },
}

pub fn start_worker(
    cmd_rx: Receiver<WorkerCommand>,
    evt_tx: Sender<WorkerEvent>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || worker_loop(cmd_rx, evt_tx))
}

fn worker_loop(cmd_rx: Receiver<WorkerCommand>, evt_tx: Sender<WorkerEvent>) {
    let mut client = IpcClient::new();
    let mut last_status_at = Instant::now() - Duration::from_secs(60);

    let syntaxes = SyntaxSet::load_defaults_newlines();
    let themes = ThemeSet::load_defaults();
    let theme = pick_theme(&themes);

    #[derive(Debug)]
    struct PendingSearch {
        id: u64,
        query: String,
        limit: usize,
        view: ViewKind,
        scope: Option<std::path::PathBuf>,
        niyamas: Vec<Niyama>,
    }

    let mut pending_search: Option<PendingSearch> = None;
    let mut pending_preview: Option<(u64, String)> = None;

    loop {
        // Receive at least one command, but wake periodically for status.
        match cmd_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(cmd) => match cmd {
                WorkerCommand::Search {
                    id,
                    query,
                    limit,
                    view,
                    scope,
                    niyamas,
                } => {
                    pending_search = Some(PendingSearch {
                        id,
                        query,
                        limit,
                        view,
                        scope,
                        niyamas,
                    })
                }
                WorkerCommand::Preview { id, path } => pending_preview = Some((id, path)),
                WorkerCommand::Quit => break,
            },
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }

        // Coalesce bursts: keep only the latest search/preview request.
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                WorkerCommand::Search {
                    id,
                    query,
                    limit,
                    view,
                    scope,
                    niyamas,
                } => {
                    pending_search = Some(PendingSearch {
                        id,
                        query,
                        limit,
                        view,
                        scope,
                        niyamas,
                    })
                }
                WorkerCommand::Preview { id, path } => pending_preview = Some((id, path)),
                WorkerCommand::Quit => return,
            }
        }

        // Periodic status updates (best-effort).
        if last_status_at.elapsed() >= Duration::from_secs(2) {
            let status = client.status().ok();
            let _ = evt_tx.send(WorkerEvent::Status { status });
            last_status_at = Instant::now();
        }

        if let Some(PendingSearch {
            id,
            query,
            limit,
            view,
            scope,
            niyamas,
        }) = pending_search.take()
        {
            let trimmed = query.trim().to_string();
            if trimmed.is_empty() {
                let _ = evt_tx.send(WorkerEvent::SearchResults {
                    id,
                    results: Vec::new(),
                    error: None,
                });
            } else {
                let scope = scope.as_deref();
                let mut results = match client.search(&trimmed, limit, scope) {
                    Ok(r) => r,
                    Err(e) => {
                        client.reconnect();
                        let _ = evt_tx.send(WorkerEvent::SearchResults {
                            id,
                            results: Vec::new(),
                            error: Some(format!("Search error: {}", e)),
                        });
                        continue;
                    }
                };

                // Scope + Niyama filtering (best-effort).
                results.retain(|r| matches_filters(r, view, scope, &niyamas));

                let _ = evt_tx.send(WorkerEvent::SearchResults {
                    id,
                    results,
                    error: None,
                });
            }
        }

        if let Some((id, path)) = pending_preview.take() {
            let (title, lines, truncated, error) = build_preview(&path, &syntaxes, theme);
            let _ = evt_tx.send(WorkerEvent::PreviewReady {
                id,
                path,
                title,
                lines,
                truncated,
            });
            if let Some(error) = error {
                tracing::debug!("Preview error: {}", error);
            }
        }
    }
}

fn matches_filters(
    result: &SearchResult,
    view: ViewKind,
    scope: Option<&std::path::Path>,
    niyamas: &[Niyama],
) -> bool {
    let path = std::path::Path::new(&result.path);

    if let Some(scope) = scope {
        if !path.starts_with(scope) {
            return false;
        }
    }

    let needs_kind =
        view == ViewKind::Sthana || niyamas.iter().any(|n| matches!(n, Niyama::Type { .. }));
    let mut kind: Option<NiyamaType> = None;
    if needs_kind {
        kind = std::fs::metadata(path).ok().map(|m| {
            if m.is_dir() {
                NiyamaType::Dir
            } else {
                NiyamaType::File
            }
        });
    }

    if view == ViewKind::Sthana && kind != Some(NiyamaType::Dir) {
        return false;
    }

    for niyama in niyamas {
        match niyama {
            Niyama::Type { kind: want, .. } => {
                if kind != Some(*want) {
                    return false;
                }
            }
            Niyama::Ext { exts, .. } => {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_lowercase());
                let Some(ext) = ext else {
                    return false;
                };
                if !exts.iter().any(|e| e == &ext) {
                    return false;
                }
            }
            Niyama::Path { needle, .. } => {
                if !result.path.to_lowercase().contains(needle) {
                    return false;
                }
            }
            Niyama::Mtime { cmp, .. } => {
                if !cmp.op.matches_i64(result.mtime, cmp.value) {
                    return false;
                }
            }
            Niyama::Size { cmp, .. } => {
                if !cmp.op.matches_u64(result.size, cmp.value) {
                    return false;
                }
            }
        }
    }

    true
}

fn pick_theme(themes: &ThemeSet) -> &Theme {
    themes
        .themes
        .get("Monokai Extended")
        .or_else(|| themes.themes.get("base16-ocean.dark"))
        .or_else(|| themes.themes.get("Solarized (dark)"))
        .or_else(|| themes.themes.values().next())
        .expect("syntect theme set must not be empty")
}

fn meta_line(text: impl Into<String>) -> StyledLine {
    vec![StyledSegment {
        text: text.into(),
        style: TextStyle {
            kind: TextKind::Meta,
            ..Default::default()
        },
    }]
}

fn error_line(text: impl Into<String>) -> StyledLine {
    vec![StyledSegment {
        text: text.into(),
        style: TextStyle {
            kind: TextKind::Error,
            bold: true,
            ..Default::default()
        },
    }]
}

fn plain_line(text: impl Into<String>) -> StyledLine {
    vec![StyledSegment {
        text: text.into(),
        style: TextStyle::default(),
    }]
}

fn build_preview(
    path: &str,
    syntaxes: &SyntaxSet,
    theme: &Theme,
) -> (String, Vec<StyledLine>, bool, Option<String>) {
    let p = std::path::Path::new(path);
    let title = p
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());

    let meta = match std::fs::metadata(p) {
        Ok(m) => m,
        Err(e) => {
            return (
                title,
                vec![error_line(format!("(unable to read metadata) {}", e))],
                false,
                Some(e.to_string()),
            );
        }
    };

    if meta.is_dir() {
        return preview_dir(p, title);
    }

    preview_file(p, title, meta.len(), syntaxes, theme)
}

fn preview_dir(
    path: &std::path::Path,
    title: String,
) -> (String, Vec<StyledLine>, bool, Option<String>) {
    const MAX_ENTRIES: usize = 200;

    let mut lines = vec![meta_line(format!("{}", path.display())), meta_line("")];

    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(e) => {
            lines.push(error_line(format!("(unable to read directory) {}", e)));
            return (title, lines, false, Some(e.to_string()));
        }
    };

    let mut shown = 0usize;
    let mut truncated = false;

    for next in entries {
        if shown >= MAX_ENTRIES {
            truncated = true;
            break;
        }

        let entry = match next {
            Ok(e) => e,
            Err(_) => continue,
        };

        let name = entry.file_name().to_string_lossy().to_string();
        let suffix = entry
            .file_type()
            .ok()
            .and_then(|ft| if ft.is_dir() { Some("/") } else { None })
            .unwrap_or("");
        lines.push(plain_line(format!("{}{}", name, suffix)));
        shown += 1;
    }

    if truncated {
        lines.push(meta_line(""));
        lines.push(meta_line(format!(
            "… (showing first {MAX_ENTRIES} entries)"
        )));
    }

    (title, lines, truncated, None)
}

fn preview_file(
    path: &std::path::Path,
    title: String,
    size: u64,
    syntaxes: &SyntaxSet,
    theme: &Theme,
) -> (String, Vec<StyledLine>, bool, Option<String>) {
    const MAX_BYTES: usize = 256 * 1024;
    const MAX_LINES: usize = 4000;

    let mut lines = vec![
        meta_line(format!("{}", path.display())),
        meta_line(format!("{} bytes", size)),
        meta_line(""),
    ];

    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            lines.push(error_line(format!("(unable to open file) {}", e)));
            return (title, lines, false, Some(e.to_string()));
        }
    };

    use std::io::Read;
    let mut buf = vec![0u8; MAX_BYTES];
    let read = match file.read(&mut buf) {
        Ok(n) => n,
        Err(e) => {
            lines.push(error_line(format!("(unable to read file) {}", e)));
            return (title, lines, false, Some(e.to_string()));
        }
    };
    buf.truncate(read);

    if buf.contains(&0) {
        lines.push(meta_line("(binary file preview)"));
        return (title, lines, true, None);
    }

    let text = String::from_utf8_lossy(&buf);
    let mut truncated_lines = false;

    let syntax = find_syntax(path, &text, syntaxes);
    let mut highlighter = syntax.map(|s| HighlightLines::new(s, theme));

    for (i, raw_line) in LinesWithEndings::from(text.as_ref()).enumerate() {
        if i >= MAX_LINES {
            truncated_lines = true;
            break;
        }

        if let Some(ref mut highlighter) = highlighter {
            let sanitized = sanitize_line(raw_line);
            match highlighter.highlight_line(&sanitized, syntaxes) {
                Ok(ranges) => {
                    let mut out = Vec::with_capacity(ranges.len().max(1));
                    for (style, fragment) in ranges {
                        let fragment = strip_line_endings(fragment);
                        if fragment.is_empty() {
                            continue;
                        }
                        out.push(StyledSegment {
                            text: fragment.to_string(),
                            style: syntect_style_to_text_style(style),
                        });
                    }

                    if out.is_empty() {
                        lines.push(plain_line(""));
                    } else {
                        lines.push(out);
                    }
                }
                Err(_) => {
                    lines.push(plain_line(strip_line_endings(&sanitized)));
                }
            }
        } else {
            let sanitized = sanitize_line(raw_line);
            lines.push(plain_line(strip_line_endings(&sanitized)));
        }
    }

    let truncated_bytes = read >= MAX_BYTES;
    let truncated = truncated_bytes || truncated_lines;

    if truncated {
        lines.push(meta_line(""));
        lines.push(meta_line("… (preview truncated)"));
    }

    (title, lines, truncated, None)
}

fn strip_line_endings(s: &str) -> &str {
    let s = s.strip_suffix('\n').unwrap_or(s);
    s.strip_suffix('\r').unwrap_or(s)
}

fn sanitize_line(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\t' => out.push_str("    "),
            '\r' => {}
            // Avoid raw ANSI/control chars affecting terminal state.
            c if c.is_control() && c != '\n' => out.push('�'),
            _ => out.push(ch),
        }
    }
    out
}

fn find_syntax<'a>(
    path: &std::path::Path,
    text: &str,
    syntaxes: &'a SyntaxSet,
) -> Option<&'a syntect::parsing::SyntaxReference> {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if let Some(syntax) = syntaxes.find_syntax_by_extension(ext) {
            return Some(syntax);
        }
    }

    let first_line = text.lines().next().unwrap_or_default();
    syntaxes.find_syntax_by_first_line(first_line)
}

fn syntect_style_to_text_style(style: syntect::highlighting::Style) -> TextStyle {
    let mut out = TextStyle {
        kind: TextKind::Normal,
        fg: Some((style.foreground.r, style.foreground.g, style.foreground.b)),
        ..Default::default()
    };

    if style.font_style.contains(FontStyle::BOLD) {
        out.bold = true;
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        out.italic = true;
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        out.underline = true;
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{CmpOp, CmpU64};
    use tempfile::tempdir;

    #[test]
    fn matches_filters_applies_scope_and_size() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("foo.rs");
        std::fs::write(&file_path, "hello").unwrap();

        let r = SearchResult {
            path: file_path.to_string_lossy().to_string(),
            name: "foo.rs".to_string(),
            score: 1.0,
            size: 5,
            mtime: 0,
        };

        let scope = dir.path();
        let niyamas = vec![Niyama::Size {
            cmp: CmpU64 {
                op: CmpOp::Gt,
                value: 1,
            },
            raw: "size:>1b".to_string(),
        }];

        assert!(matches_filters(&r, ViewKind::Patra, Some(scope), &niyamas));
    }

    #[test]
    fn matches_filters_applies_type_and_ext() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("foo.rs");
        let dir_path = dir.path().join("bar");
        std::fs::write(&file_path, "hello").unwrap();
        std::fs::create_dir_all(&dir_path).unwrap();

        let file = SearchResult {
            path: file_path.to_string_lossy().to_string(),
            name: "foo.rs".to_string(),
            score: 1.0,
            size: 0,
            mtime: 0,
        };
        let subdir = SearchResult {
            path: dir_path.to_string_lossy().to_string(),
            name: "bar".to_string(),
            score: 1.0,
            size: 0,
            mtime: 0,
        };

        let type_dir = vec![Niyama::Type {
            kind: NiyamaType::Dir,
            raw: "type:dir".to_string(),
        }];
        assert!(matches_filters(
            &subdir,
            ViewKind::Patra,
            Some(dir.path()),
            &type_dir
        ));
        assert!(!matches_filters(
            &file,
            ViewKind::Patra,
            Some(dir.path()),
            &type_dir
        ));

        let ext_rs = vec![Niyama::Ext {
            exts: vec!["rs".to_string()],
            raw: "ext:rs".to_string(),
        }];
        assert!(matches_filters(
            &file,
            ViewKind::Patra,
            Some(dir.path()),
            &ext_rs
        ));
        assert!(!matches_filters(
            &subdir,
            ViewKind::Patra,
            Some(dir.path()),
            &ext_rs
        ));
    }
}
