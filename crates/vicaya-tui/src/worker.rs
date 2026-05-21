//! Background worker for daemon IPC and preview loading.

use crate::client::{DaemonStatus, IpcClient};
use crate::state::{Niyama, NiyamaType, StyledLine, StyledSegment, TextKind, TextStyle, ViewKind};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;
use vicaya_index::SearchResult;

use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle, Theme, ThemeSet},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};

const STATUS_FAILURES_BEFORE_OFFLINE: usize = 2;
const STATUS_POLL_INTERVAL: Duration = if cfg!(test) {
    Duration::from_millis(50)
} else {
    Duration::from_secs(2)
};
const STATUS_POLL_SLEEP_STEP: Duration = if cfg!(test) {
    Duration::from_millis(10)
} else {
    Duration::from_millis(50)
};

pub enum WorkerCommand {
    Search {
        id: u64,
        query: String,
        limit: usize,
        view: ViewKind,
        boost_scope: Option<std::path::PathBuf>,
        filter_scope: Option<std::path::PathBuf>,
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
    let mut search_client = IpcClient::new();
    let status_stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let status_handle = start_status_worker(evt_tx.clone(), status_stop.clone());

    let syntaxes = SyntaxSet::load_defaults_newlines();
    let themes = ThemeSet::load_defaults();
    let theme = pick_theme(&themes);

    #[derive(Debug)]
    struct PendingSearch {
        id: u64,
        query: String,
        limit: usize,
        view: ViewKind,
        boost_scope: Option<std::path::PathBuf>,
        filter_scope: Option<std::path::PathBuf>,
        niyamas: Vec<Niyama>,
    }

    let mut pending_search: Option<PendingSearch> = None;
    let mut pending_preview: Option<(u64, String)> = None;

    'worker: loop {
        // Receive at least one command, but wake periodically for status.
        match cmd_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(cmd) => match cmd {
                WorkerCommand::Search {
                    id,
                    query,
                    limit,
                    view,
                    boost_scope,
                    filter_scope,
                    niyamas,
                } => {
                    pending_search = Some(PendingSearch {
                        id,
                        query,
                        limit,
                        view,
                        boost_scope,
                        filter_scope,
                        niyamas,
                    })
                }
                WorkerCommand::Preview { id, path } => pending_preview = Some((id, path)),
                WorkerCommand::Quit => break 'worker,
            },
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break 'worker,
        }

        // Coalesce bursts: keep only the latest search/preview request.
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                WorkerCommand::Search {
                    id,
                    query,
                    limit,
                    view,
                    boost_scope,
                    filter_scope,
                    niyamas,
                } => {
                    pending_search = Some(PendingSearch {
                        id,
                        query,
                        limit,
                        view,
                        boost_scope,
                        filter_scope,
                        niyamas,
                    })
                }
                WorkerCommand::Preview { id, path } => pending_preview = Some((id, path)),
                WorkerCommand::Quit => break 'worker,
            }
        }

        if let Some(PendingSearch {
            id,
            query,
            limit,
            view,
            boost_scope,
            filter_scope,
            niyamas,
        }) = pending_search.take()
        {
            let trimmed = query.trim().to_string();
            let filter_scope = filter_scope.as_deref();
            let boost_scope = boost_scope
                .as_ref()
                .cloned()
                .or_else(|| std::env::current_dir().ok());
            let boost_scope = boost_scope.as_deref();

            // When query is empty, request recent files from daemon
            let recent_if_empty = trimmed.is_empty();

            let mut results = match search_client.search(
                &trimmed,
                limit,
                boost_scope,
                filter_scope,
                recent_if_empty,
            ) {
                Ok(r) => r,
                Err(e) => {
                    search_client.reconnect();
                    let _ = evt_tx.send(WorkerEvent::SearchResults {
                        id,
                        results: Vec::new(),
                        error: Some(format!("Search error: {}", e)),
                    });
                    continue;
                }
            };

            // Scope + Niyama filtering (best-effort).
            results.retain(|r| matches_filters(r, view, filter_scope, &niyamas));

            let _ = evt_tx.send(WorkerEvent::SearchResults {
                id,
                results,
                error: None,
            });
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

    status_stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = status_handle.join();
}

fn start_status_worker(
    evt_tx: Sender<WorkerEvent>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut client = IpcClient::best_effort();
        let mut last_status: Option<DaemonStatus> = None;
        let mut failures = 0usize;

        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
            match client.status() {
                Ok(status) => {
                    failures = 0;
                    last_status = Some(status.clone());
                    let _ = evt_tx.send(WorkerEvent::Status {
                        status: Some(status),
                    });
                }
                Err(_) => {
                    failures = failures.saturating_add(1);
                    client.reconnect();

                    if failures >= STATUS_FAILURES_BEFORE_OFFLINE {
                        last_status = None;
                        let _ = evt_tx.send(WorkerEvent::Status { status: None });
                    } else if let Some(status) = last_status.clone() {
                        let _ = evt_tx.send(WorkerEvent::Status {
                            status: Some(status),
                        });
                    }
                }
            }

            let mut slept = Duration::from_millis(0);
            while slept < STATUS_POLL_INTERVAL {
                if stop.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(STATUS_POLL_SLEEP_STEP);
                slept += STATUS_POLL_SLEEP_STEP;
            }
        }
    })
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
    use std::io::{BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;
    use tempfile::tempdir;
    use vicaya_core::ipc::{BuildInfo, Request, Response};

    fn result(path: &std::path::Path, name: &str, size: u64, mtime: i64) -> SearchResult {
        SearchResult {
            path: path.to_string_lossy().to_string(),
            name: name.to_string(),
            score: 1.0,
            size,
            mtime,
        }
    }

    fn test_syntaxes_and_theme() -> (SyntaxSet, ThemeSet) {
        (
            SyntaxSet::load_defaults_newlines(),
            ThemeSet::load_defaults(),
        )
    }

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

    #[test]
    fn matches_filters_rejects_out_of_scope_path_filters_and_mtime() {
        let dir = tempdir().unwrap();
        let in_scope = dir.path().join("src").join("main.rs");
        let out_scope = dir.path().join("target").join("main.rs");
        std::fs::create_dir_all(in_scope.parent().unwrap()).unwrap();
        std::fs::create_dir_all(out_scope.parent().unwrap()).unwrap();
        std::fs::write(&in_scope, "fn main() {}\n").unwrap();
        std::fs::write(&out_scope, "fn main() {}\n").unwrap();

        let niyamas = vec![
            Niyama::Path {
                needle: "src".to_string(),
                raw: "path:src".to_string(),
            },
            Niyama::Mtime {
                cmp: crate::state::CmpI64 {
                    op: CmpOp::Gte,
                    value: 100,
                },
                raw: "mtime:>=1970-01-01".to_string(),
            },
        ];

        assert!(matches_filters(
            &result(&in_scope, "main.rs", 13, 101),
            ViewKind::Patra,
            Some(dir.path()),
            &niyamas
        ));
        assert!(!matches_filters(
            &result(&out_scope, "main.rs", 13, 101),
            ViewKind::Patra,
            Some(dir.path()),
            &niyamas
        ));
        assert!(!matches_filters(
            &result(&in_scope, "main.rs", 13, 99),
            ViewKind::Patra,
            Some(dir.path()),
            &niyamas
        ));
    }

    #[test]
    fn preview_file_sanitizes_controls_and_assigns_highlight_styles() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("main.rs");
        std::fs::write(&file, "fn main() {\n\tprintln!(\"hi\");\n\u{1b}[31m\n}\n").unwrap();
        let (syntaxes, themes) = test_syntaxes_and_theme();
        let theme = pick_theme(&themes);

        let (title, lines, truncated, error) =
            build_preview(file.to_str().unwrap(), &syntaxes, theme);

        assert_eq!(title, "main.rs");
        assert!(!truncated);
        assert!(error.is_none());
        let rendered = lines
            .iter()
            .flat_map(|line| line.iter().map(|seg| seg.text.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("println!"));
        assert!(rendered.contains("    "));
        assert!(lines.iter().flatten().any(|seg| seg.style.fg.is_some()));
    }

    #[test]
    fn preview_binary_file_is_marked_truncated_without_decoding() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("archive.bin");
        std::fs::write(&file, b"abc\0def").unwrap();
        let (syntaxes, themes) = test_syntaxes_and_theme();
        let theme = pick_theme(&themes);

        let (title, lines, truncated, error) =
            build_preview(file.to_str().unwrap(), &syntaxes, theme);

        assert_eq!(title, "archive.bin");
        assert!(truncated);
        assert!(error.is_none());
        assert!(lines
            .iter()
            .flatten()
            .any(|seg| seg.text.contains("binary file preview")));
    }

    #[test]
    fn preview_directory_lists_entries_and_marks_directories() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("README.md"), "readme").unwrap();
        let (syntaxes, themes) = test_syntaxes_and_theme();
        let theme = pick_theme(&themes);

        let (_title, lines, truncated, error) =
            build_preview(dir.path().to_str().unwrap(), &syntaxes, theme);

        assert!(!truncated);
        assert!(error.is_none());
        let rendered = lines
            .iter()
            .flat_map(|line| line.iter().map(|seg| seg.text.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("README.md"));
        assert!(rendered.contains("src/"));
    }

    #[test]
    fn preview_directory_truncates_large_listing() {
        let dir = tempdir().unwrap();
        for i in 0..205 {
            std::fs::write(dir.path().join(format!("file-{i:03}.txt")), "").unwrap();
        }
        let (syntaxes, themes) = test_syntaxes_and_theme();
        let theme = pick_theme(&themes);

        let (_title, lines, truncated, error) =
            build_preview(dir.path().to_str().unwrap(), &syntaxes, theme);

        assert!(truncated);
        assert!(error.is_none());
        assert!(lines
            .iter()
            .flatten()
            .any(|seg| seg.text.contains("showing first 200 entries")));
    }

    #[test]
    fn preview_missing_path_returns_error_line() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("missing.txt");
        let (syntaxes, themes) = test_syntaxes_and_theme();
        let theme = pick_theme(&themes);

        let (_title, lines, truncated, error) =
            build_preview(missing.to_str().unwrap(), &syntaxes, theme);

        assert!(!truncated);
        assert!(error.is_some());
        assert!(lines.iter().flatten().any(|seg| {
            seg.style.kind == TextKind::Error && seg.text.contains("unable to read metadata")
        }));
    }

    fn start_fake_daemon(
        vicaya_dir: &std::path::Path,
        stop: Arc<AtomicBool>,
    ) -> std::thread::JoinHandle<Vec<Request>> {
        let socket = vicaya_dir.join("daemon.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();

        std::thread::spawn(move || {
            let mut requests = Vec::new();
            while !stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        let mut reader = BufReader::new(stream.try_clone().unwrap());
                        let line = vicaya_core::ipc::read_message(&mut reader)
                            .unwrap()
                            .unwrap();
                        let request = Request::from_json(&line).unwrap();
                        let response = match &request {
                            Request::Status => Response::Status {
                                pid: 77,
                                build: BuildInfo {
                                    version: "1.2.0".to_string(),
                                    git_sha: "abc1234".to_string(),
                                    timestamp: "2026-05-19T00:00:00Z".to_string(),
                                    target: "aarch64-apple-darwin".to_string(),
                                },
                                indexed_files: 3,
                                trigram_count: 9,
                                arena_size: 128,
                                index_allocated_bytes: 256,
                                state_allocated_bytes: 512,
                                last_updated: 1_700_000_000,
                                reconciling: false,
                            },
                            Request::Search { .. } => Response::SearchResults {
                                results: vec![
                                    vicaya_core::ipc::SearchResult {
                                        path: "/tmp/repo/src/main.rs".to_string(),
                                        name: "main.rs".to_string(),
                                        score: 1.0,
                                        size: 12,
                                        mtime: 1_700_000_000,
                                    },
                                    vicaya_core::ipc::SearchResult {
                                        path: "/tmp/repo/target/main.rs".to_string(),
                                        name: "main.rs".to_string(),
                                        score: 0.5,
                                        size: 12,
                                        mtime: 1_700_000_000,
                                    },
                                ],
                            },
                            _ => Response::Ok,
                        };
                        requests.push(request);
                        let mut json = response.to_json().unwrap();
                        json.push('\n');
                        let _ = stream.write_all(json.as_bytes());
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
            requests
        })
    }

    fn start_status_blackhole_daemon(
        vicaya_dir: &std::path::Path,
        stop: Arc<AtomicBool>,
    ) -> std::thread::JoinHandle<Vec<Request>> {
        let socket = vicaya_dir.join("daemon.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));

        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        let requests = Arc::clone(&requests);
                        let stop = Arc::clone(&stop);
                        std::thread::spawn(move || {
                            let mut reader = BufReader::new(stream.try_clone().unwrap());
                            let Ok(Some(line)) = vicaya_core::ipc::read_message(&mut reader) else {
                                return;
                            };
                            let Ok(request) = Request::from_json(&line) else {
                                return;
                            };
                            requests.lock().unwrap().push(request.clone());

                            match request {
                                Request::Status => {
                                    while !stop.load(Ordering::Relaxed) {
                                        std::thread::sleep(Duration::from_millis(25));
                                    }
                                }
                                Request::Search { .. } => {
                                    let response = Response::SearchResults {
                                        results: vec![vicaya_core::ipc::SearchResult {
                                            path: "/tmp/repo/src/main.rs".to_string(),
                                            name: "main.rs".to_string(),
                                            score: 1.0,
                                            size: 12,
                                            mtime: 1_700_000_000,
                                        }],
                                    };
                                    let mut json = response.to_json().unwrap();
                                    json.push('\n');
                                    stream.write_all(json.as_bytes()).unwrap();
                                }
                                _ => {}
                            }
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }

            requests.lock().unwrap().clone()
        })
    }

    fn start_one_missed_status_daemon(
        vicaya_dir: &std::path::Path,
        stop: Arc<AtomicBool>,
    ) -> std::thread::JoinHandle<Vec<Request>> {
        let socket = vicaya_dir.join("daemon.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let status_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        let requests = Arc::clone(&requests);
                        let status_count = Arc::clone(&status_count);
                        std::thread::spawn(move || {
                            let mut reader = BufReader::new(stream.try_clone().unwrap());
                            while let Ok(Some(line)) = vicaya_core::ipc::read_message(&mut reader) {
                                let Ok(request) = Request::from_json(&line) else {
                                    return;
                                };
                                requests.lock().unwrap().push(request.clone());

                                let response = match request {
                                    Request::Status => {
                                        let count =
                                            status_count.fetch_add(1, Ordering::Relaxed) + 1;
                                        if count == 2 {
                                            return;
                                        }

                                        Response::Status {
                                            pid: 77,
                                            build: BuildInfo {
                                                version: "1.2.0".to_string(),
                                                git_sha: "abc1234".to_string(),
                                                timestamp: "2026-05-19T00:00:00Z".to_string(),
                                                target: "aarch64-apple-darwin".to_string(),
                                            },
                                            indexed_files: 3,
                                            trigram_count: 9,
                                            arena_size: 128,
                                            index_allocated_bytes: 256,
                                            state_allocated_bytes: 512,
                                            last_updated: 1_700_000_000,
                                            reconciling: false,
                                        }
                                    }
                                    _ => Response::Ok,
                                };

                                let mut json = response.to_json().unwrap();
                                json.push('\n');
                                let _ = stream.write_all(json.as_bytes());
                            }
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }

            requests.lock().unwrap().clone()
        })
    }

    #[test]
    fn worker_loop_coalesces_searches_reports_status_and_builds_preview() {
        let _lock = vicaya_core::paths::test_env_lock();
        let vicaya_dir = tempdir().unwrap();
        std::env::set_var("VICAYA_DIR", vicaya_dir.path());
        let stop = Arc::new(AtomicBool::new(false));
        let fake_daemon = start_fake_daemon(vicaya_dir.path(), stop.clone());

        let preview_dir = tempdir().unwrap();
        let preview_file = preview_dir.path().join("main.rs");
        std::fs::write(&preview_file, "fn main() {}\n").unwrap();

        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let (evt_tx, evt_rx) = std::sync::mpsc::channel();

        cmd_tx
            .send(WorkerCommand::Search {
                id: 1,
                query: "stale".to_string(),
                limit: 10,
                view: ViewKind::Patra,
                boost_scope: Some(std::path::PathBuf::from("/tmp/repo")),
                filter_scope: Some(std::path::PathBuf::from("/tmp/repo/src")),
                niyamas: Vec::new(),
            })
            .unwrap();
        cmd_tx
            .send(WorkerCommand::Search {
                id: 2,
                query: "main".to_string(),
                limit: 10,
                view: ViewKind::Patra,
                boost_scope: Some(std::path::PathBuf::from("/tmp/repo")),
                filter_scope: Some(std::path::PathBuf::from("/tmp/repo/src")),
                niyamas: vec![Niyama::Path {
                    needle: "src".to_string(),
                    raw: "path:src".to_string(),
                }],
            })
            .unwrap();
        cmd_tx
            .send(WorkerCommand::Preview {
                id: 9,
                path: preview_file.to_string_lossy().to_string(),
            })
            .unwrap();

        let worker = start_worker(cmd_rx, evt_tx);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut saw_status = false;
        let mut saw_search = false;
        let mut saw_preview = false;
        while std::time::Instant::now() < deadline {
            if let Ok(event) = evt_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                match event {
                    WorkerEvent::Status { status } => {
                        saw_status = status
                            .as_ref()
                            .is_some_and(|status| status.indexed_files == 3);
                    }
                    WorkerEvent::SearchResults { id, results, error } => {
                        if id == 2 {
                            assert!(error.is_none());
                            assert_eq!(results.len(), 1);
                            assert!(results[0].path.contains("/src/"));
                            saw_search = true;
                        }
                    }
                    WorkerEvent::PreviewReady {
                        id,
                        title,
                        lines,
                        truncated,
                        ..
                    } => {
                        if id == 9 {
                            assert_eq!(title, "main.rs");
                            assert!(!truncated);
                            let rendered = lines
                                .iter()
                                .flatten()
                                .map(|seg| seg.text.as_str())
                                .collect::<Vec<_>>()
                                .join("");
                            assert!(rendered.contains("main"));
                            saw_preview = true;
                        }
                    }
                }
            }
            if saw_status && saw_search && saw_preview {
                break;
            }
        }

        cmd_tx.send(WorkerCommand::Quit).unwrap();
        worker.join().unwrap();
        stop.store(true, Ordering::Relaxed);
        let requests = fake_daemon.join().unwrap();

        assert!(saw_status, "worker did not report daemon status");
        assert!(saw_search, "worker did not report latest search results");
        assert!(saw_preview, "worker did not report preview");
        assert!(requests.iter().any(|req| matches!(req, Request::Status)));
        assert!(requests
            .iter()
            .any(|req| { matches!(req, Request::Search { query, .. } if query == "main") }));
        assert!(!requests
            .iter()
            .any(|req| { matches!(req, Request::Search { query, .. } if query == "stale") }));
    }

    #[test]
    fn worker_search_is_not_blocked_by_hung_status_connection() {
        let _lock = vicaya_core::paths::test_env_lock();
        let vicaya_dir = tempdir().unwrap();
        std::env::set_var("VICAYA_DIR", vicaya_dir.path());
        let stop = Arc::new(AtomicBool::new(false));
        let fake_daemon = start_status_blackhole_daemon(vicaya_dir.path(), Arc::clone(&stop));

        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let (evt_tx, evt_rx) = std::sync::mpsc::channel();
        let worker = start_worker(cmd_rx, evt_tx);

        std::thread::sleep(Duration::from_millis(250));
        cmd_tx
            .send(WorkerCommand::Search {
                id: 1,
                query: "main".to_string(),
                limit: 10,
                view: ViewKind::Patra,
                boost_scope: Some(std::path::PathBuf::from("/tmp/repo")),
                filter_scope: Some(std::path::PathBuf::from("/tmp/repo/src")),
                niyamas: Vec::new(),
            })
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut saw_search = false;
        while Instant::now() < deadline {
            if let Ok(WorkerEvent::SearchResults { id, results, error }) =
                evt_rx.recv_timeout(Duration::from_millis(100))
            {
                if id == 1 {
                    assert!(error.is_none(), "unexpected search error: {error:?}");
                    assert_eq!(results.len(), 1);
                    assert_eq!(results[0].name, "main.rs");
                    saw_search = true;
                    break;
                }
            }
        }

        cmd_tx.send(WorkerCommand::Quit).unwrap();
        worker.join().unwrap();
        stop.store(true, Ordering::Relaxed);
        let requests = fake_daemon.join().unwrap();

        assert!(saw_search, "search was blocked behind status polling");
        assert!(requests.iter().any(|req| matches!(req, Request::Status)));
        assert!(requests
            .iter()
            .any(|req| matches!(req, Request::Search { query, .. } if query == "main")));
    }

    #[test]
    fn status_worker_keeps_last_status_after_one_missed_probe() {
        let _lock = vicaya_core::paths::test_env_lock();
        let vicaya_dir = tempdir().unwrap();
        std::env::set_var("VICAYA_DIR", vicaya_dir.path());
        let stop = Arc::new(AtomicBool::new(false));
        let fake_daemon = start_one_missed_status_daemon(vicaya_dir.path(), Arc::clone(&stop));

        let (evt_tx, evt_rx) = std::sync::mpsc::channel();
        let status_worker = start_status_worker(evt_tx, Arc::clone(&stop));

        let deadline = Instant::now() + Duration::from_secs(3);
        let mut statuses = Vec::new();
        while Instant::now() < deadline && statuses.len() < 3 {
            if let Ok(WorkerEvent::Status { status }) =
                evt_rx.recv_timeout(Duration::from_millis(100))
            {
                statuses.push(status.map(|status| status.indexed_files));
            }
        }

        stop.store(true, Ordering::Relaxed);
        status_worker.join().unwrap();
        let requests = fake_daemon.join().unwrap();

        assert!(
            statuses.len() >= 3,
            "expected status events before and after missed probe, got {statuses:?}"
        );
        assert!(
            statuses.iter().all(|status| *status == Some(3)),
            "single missed probe should not report daemon offline: {statuses:?}"
        );
        assert!(
            requests
                .iter()
                .filter(|req| matches!(req, Request::Status))
                .count()
                >= 3
        );
    }
}
