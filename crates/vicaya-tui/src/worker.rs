//! Background worker for daemon IPC and preview loading.

use crate::client::{DaemonStatus, IpcClient};
use crate::state::ViewKind;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};
use vicaya_index::SearchResult;

pub enum WorkerCommand {
    Search {
        id: u64,
        query: String,
        limit: usize,
        view: ViewKind,
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
        lines: Vec<String>,
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

    let mut pending_search: Option<(u64, String, usize, ViewKind)> = None;
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
                } => pending_search = Some((id, query, limit, view)),
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
                } => pending_search = Some((id, query, limit, view)),
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

        if let Some((id, query, limit, view)) = pending_search.take() {
            let trimmed = query.trim().to_string();
            if trimmed.is_empty() {
                let _ = evt_tx.send(WorkerEvent::SearchResults {
                    id,
                    results: Vec::new(),
                    error: None,
                });
            } else {
                let mut results = match client.search(&trimmed, limit) {
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

                // Drishti-specific filtering (best-effort).
                if view == ViewKind::Sthana {
                    results.retain(|r| {
                        std::fs::metadata(&r.path)
                            .map(|m| m.is_dir())
                            .unwrap_or(false)
                    });
                }

                let _ = evt_tx.send(WorkerEvent::SearchResults {
                    id,
                    results,
                    error: None,
                });
            }
        }

        if let Some((id, path)) = pending_preview.take() {
            let (title, lines, truncated, error) = build_preview(&path);
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

fn build_preview(path: &str) -> (String, Vec<String>, bool, Option<String>) {
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
                vec![format!("(unable to read metadata) {}", e)],
                false,
                Some(e.to_string()),
            );
        }
    };

    if meta.is_dir() {
        return preview_dir(p, title);
    }

    preview_file(p, title, meta.len())
}

fn preview_dir(
    path: &std::path::Path,
    title: String,
) -> (String, Vec<String>, bool, Option<String>) {
    const MAX_ENTRIES: usize = 200;

    let mut lines = vec![format!("{}", path.display()), "".to_string()];

    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(e) => {
            lines.push(format!("(unable to read directory) {}", e));
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
        lines.push(format!("{}{}", name, suffix));
        shown += 1;
    }

    if truncated {
        lines.push("".to_string());
        lines.push(format!("… (showing first {MAX_ENTRIES} entries)"));
    }

    (title, lines, truncated, None)
}

fn preview_file(
    path: &std::path::Path,
    title: String,
    size: u64,
) -> (String, Vec<String>, bool, Option<String>) {
    const MAX_BYTES: usize = 256 * 1024;
    const MAX_LINES: usize = 4000;

    let mut lines = vec![
        format!("{}", path.display()),
        format!("{} bytes", size),
        "".to_string(),
    ];

    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            lines.push(format!("(unable to open file) {}", e));
            return (title, lines, false, Some(e.to_string()));
        }
    };

    use std::io::Read;
    let mut buf = vec![0u8; MAX_BYTES];
    let read = match file.read(&mut buf) {
        Ok(n) => n,
        Err(e) => {
            lines.push(format!("(unable to read file) {}", e));
            return (title, lines, false, Some(e.to_string()));
        }
    };
    buf.truncate(read);

    if buf.contains(&0) {
        lines.push("(binary file preview)".to_string());
        return (title, lines, true, None);
    }

    let text = String::from_utf8_lossy(&buf);
    let mut truncated_lines = false;

    for (i, line) in text.lines().enumerate() {
        if i >= MAX_LINES {
            truncated_lines = true;
            break;
        }
        lines.push(line.to_string());
    }

    let truncated_bytes = read >= MAX_BYTES;
    let truncated = truncated_bytes || truncated_lines;

    if truncated {
        lines.push("".to_string());
        lines.push("… (preview truncated)".to_string());
    }

    (title, lines, truncated, None)
}
