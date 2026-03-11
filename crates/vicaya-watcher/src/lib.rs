//! vicaya-watcher: FSEvents-based file watcher.

use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::mpsc::{channel, Receiver};
use tracing::{debug, info};
use vicaya_core::Result;

/// Events that update the index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IndexUpdate {
    /// A new file was created.
    Create { path: String },
    /// A file was modified.
    Modify { path: String },
    /// A file was deleted.
    Delete { path: String },
    /// A file was moved/renamed.
    Move { from: String, to: String },
}

/// File system watcher.
pub struct FileWatcher {
    _watcher: RecommendedWatcher,
    receiver: Receiver<notify::Result<Event>>,
}

impl FileWatcher {
    /// Create a new file watcher for the given paths.
    pub fn new<P: AsRef<Path>>(paths: &[P]) -> Result<Self> {
        let (tx, rx) = channel();

        let mut watcher = RecommendedWatcher::new(tx, Config::default())
            .map_err(|e| vicaya_core::Error::Watcher(e.to_string()))?;

        for path in paths {
            info!("Watching path: {}", path.as_ref().display());
            watcher
                .watch(path.as_ref(), RecursiveMode::Recursive)
                .map_err(|e| vicaya_core::Error::Watcher(e.to_string()))?;
        }

        Ok(Self {
            _watcher: watcher,
            receiver: rx,
        })
    }

    /// Get the next batch of index updates (non-blocking).
    pub fn poll_updates(&self) -> Vec<IndexUpdate> {
        let mut updates = Vec::new();

        while let Ok(Ok(event)) = self.receiver.try_recv() {
            debug!("File event: {:?}", event);
            updates.extend(Self::event_to_updates(event));
        }

        updates
    }

    /// Convert a notify event to index updates.
    fn event_to_updates(event: Event) -> Vec<IndexUpdate> {
        use notify::event::{ModifyKind, RenameMode};
        use notify::EventKind;

        match event.kind {
            EventKind::Create(_) => event
                .paths
                .into_iter()
                .map(|p| IndexUpdate::Create {
                    path: p.to_string_lossy().to_string(),
                })
                .collect(),
            EventKind::Modify(ModifyKind::Name(rename_mode)) => match rename_mode {
                RenameMode::From => event
                    .paths
                    .into_iter()
                    .map(|p| IndexUpdate::Delete {
                        path: p.to_string_lossy().to_string(),
                    })
                    .collect(),
                RenameMode::To => event
                    .paths
                    .into_iter()
                    .map(|p| IndexUpdate::Create {
                        path: p.to_string_lossy().to_string(),
                    })
                    .collect(),
                RenameMode::Both => Self::ordered_move_update(event.paths),
                RenameMode::Any | RenameMode::Other => Self::heuristic_move_update(event.paths),
            },
            EventKind::Modify(_) => event
                .paths
                .into_iter()
                .map(|p| IndexUpdate::Modify {
                    path: p.to_string_lossy().to_string(),
                })
                .collect(),
            EventKind::Remove(_) => event
                .paths
                .into_iter()
                .map(|p| IndexUpdate::Delete {
                    path: p.to_string_lossy().to_string(),
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn ordered_move_update(paths: Vec<std::path::PathBuf>) -> Vec<IndexUpdate> {
        match paths.as_slice() {
            [from, to] => vec![IndexUpdate::Move {
                from: from.to_string_lossy().to_string(),
                to: to.to_string_lossy().to_string(),
            }],
            _ => Self::best_effort_modify_updates(paths),
        }
    }

    fn heuristic_move_update(paths: Vec<std::path::PathBuf>) -> Vec<IndexUpdate> {
        match paths.as_slice() {
            [first, second] => {
                let (from, to) = match (first.exists(), second.exists()) {
                    (false, true) => (first, second),
                    (true, false) => (second, first),
                    _ => (first, second),
                };

                vec![IndexUpdate::Move {
                    from: from.to_string_lossy().to_string(),
                    to: to.to_string_lossy().to_string(),
                }]
            }
            _ => Self::best_effort_modify_updates(paths),
        }
    }

    fn best_effort_modify_updates(paths: Vec<std::path::PathBuf>) -> Vec<IndexUpdate> {
        // Some backends may emit a rename without both endpoints. Upsert whatever
        // paths we have as a best-effort; the daemon can dedupe by inode.
        paths
            .into_iter()
            .map(|p| IndexUpdate::Modify {
                path: p.to_string_lossy().to_string(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{ModifyKind, RenameMode};
    use notify::EventKind;

    #[test]
    fn rename_both_trusts_ordered_pair() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("old_name.txt");
        let to = dir.path().join("new_name.txt");

        std::fs::write(&from, "").unwrap();
        std::fs::write(&to, "").unwrap();

        let event = notify::Event {
            kind: EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
            paths: vec![from.clone(), to.clone()],
            attrs: Default::default(),
        };

        let updates = FileWatcher::event_to_updates(event);
        let from_str = from.to_string_lossy().to_string();
        let to_str = to.to_string_lossy().to_string();

        assert_eq!(updates.len(), 1);
        assert!(
            matches!(
                &updates[0],
                IndexUpdate::Move { from: f, to: t } if f == &from_str && t == &to_str
            ),
            "expected Move from={} to={}, got: {:?}",
            from.display(),
            to.display(),
            updates
        );
    }

    #[test]
    fn rename_both_preserves_reported_order_even_if_paths_look_reversed() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("old_name.txt");
        let to = dir.path().join("new_name.txt");

        // Simulate post-rename state: destination exists, source does not. For
        // RenameMode::Both we still trust the ordered pair supplied by notify.
        std::fs::write(&to, "").unwrap();

        let event = notify::Event {
            kind: EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
            paths: vec![to.clone(), from.clone()],
            attrs: Default::default(),
        };

        let updates = FileWatcher::event_to_updates(event);
        let reported_from = to.to_string_lossy().to_string();
        let reported_to = from.to_string_lossy().to_string();

        assert_eq!(updates.len(), 1);
        assert!(
            matches!(
                &updates[0],
                IndexUpdate::Move { from, to } if from == &reported_from && to == &reported_to
            ),
            "expected Move from={} to={} without heuristic correction, got: {:?}",
            to.display(),
            from.display(),
            updates
        );
    }

    #[test]
    fn rename_any_uses_existence_heuristic_for_direction() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("old_name.txt");
        let to = dir.path().join("new_name.txt");

        std::fs::write(&to, "").unwrap();

        let event = notify::Event {
            kind: EventKind::Modify(ModifyKind::Name(RenameMode::Any)),
            paths: vec![to.clone(), from.clone()],
            attrs: Default::default(),
        };

        let updates = FileWatcher::event_to_updates(event);
        let from_str = from.to_string_lossy().to_string();
        let to_str = to.to_string_lossy().to_string();

        assert_eq!(updates.len(), 1);
        assert!(
            matches!(
                &updates[0],
                IndexUpdate::Move { from: f, to: t } if f == &from_str && t == &to_str
            ),
            "expected heuristic Move from={} to={}, got: {:?}",
            from.display(),
            to.display(),
            updates
        );
    }

    #[test]
    fn rename_other_with_ambiguous_paths_falls_back_to_modify() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.txt");
        let second = dir.path().join("second.txt");
        let third = dir.path().join("third.txt");

        let event = notify::Event {
            kind: EventKind::Modify(ModifyKind::Name(RenameMode::Other)),
            paths: vec![first.clone(), second.clone(), third.clone()],
            attrs: Default::default(),
        };

        let updates = FileWatcher::event_to_updates(event);
        let expected: Vec<String> = vec![first, second, third]
            .into_iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect();

        assert_eq!(updates.len(), 3);
        for (update, path) in updates.iter().zip(expected.iter()) {
            assert!(
                matches!(update, IndexUpdate::Modify { path: candidate } if candidate == path),
                "expected Modify for {}, got: {:?}",
                path,
                update
            );
        }
    }
}
