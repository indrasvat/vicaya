//! vicaya-watcher: FSEvents-based file watcher.

use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc::{channel, Receiver};
use tracing::{debug, info};
use vicaya_core::Result;

/// Events that update the index.
#[derive(Debug, Clone)]
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
            if let Some(update) = self.event_to_update(event) {
                updates.push(update);
            }
        }

        updates
    }

    /// Convert a notify event to an index update.
    fn event_to_update(&self, event: Event) -> Option<IndexUpdate> {
        use notify::EventKind;

        match event.kind {
            EventKind::Create(_) => {
                let path = event.paths.first()?.to_string_lossy().to_string();
                Some(IndexUpdate::Create { path })
            }
            EventKind::Modify(_) => {
                let path = event.paths.first()?.to_string_lossy().to_string();
                Some(IndexUpdate::Modify { path })
            }
            EventKind::Remove(_) => {
                let path = event.paths.first()?.to_string_lossy().to_string();
                Some(IndexUpdate::Delete { path })
            }
            EventKind::Any => {
                // Handle rename/move events
                if event.paths.len() == 2 {
                    Some(IndexUpdate::Move {
                        from: event.paths[0].to_string_lossy().to_string(),
                        to: event.paths[1].to_string_lossy().to_string(),
                    })
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}
