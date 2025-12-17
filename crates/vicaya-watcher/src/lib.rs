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
                RenameMode::Both | RenameMode::Any | RenameMode::Other => {
                    let paths = event.paths;
                    if paths.len() == 2 {
                        let mut paths = paths;
                        let second = paths.pop().unwrap();
                        let first = paths.pop().unwrap();

                        let (from, to) = match (first.exists(), second.exists()) {
                            (false, true) => (first, second),
                            (true, false) => (second, first),
                            _ => (first, second),
                        };

                        vec![IndexUpdate::Move {
                            from: from.to_string_lossy().to_string(),
                            to: to.to_string_lossy().to_string(),
                        }]
                    } else {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{ModifyKind, RenameMode};
    use notify::EventKind;

    #[test]
    fn rename_both_uses_existing_path_as_destination() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("old_name.txt");
        let to = dir.path().join("new_name.txt");

        // Simulate post-rename state: destination exists, source does not.
        std::fs::write(&to, "").unwrap();

        let event = notify::Event {
            kind: EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
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
            "expected Move from={} to={}, got: {:?}",
            from.display(),
            to.display(),
            updates
        );
    }
}
