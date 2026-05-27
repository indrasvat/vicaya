//! Local usage memory for Smriti (recent/frecency) ranking.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const CURRENT_VERSION: u16 = 1;

/// A user action that can teach vicaya which paths are useful.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmritiAction {
    /// Opened a file in the configured editor.
    Open,
    /// Copied a path to the clipboard.
    Copy,
    /// Revealed a path in the platform file manager.
    Reveal,
    /// Printed a path for shell consumption.
    Print,
    /// Entered a directory as the active TUI scope.
    Enter,
}

impl SmritiAction {
    /// Return the stable lowercase action name used in human-facing output.
    pub fn as_str(self) -> &'static str {
        match self {
            SmritiAction::Open => "open",
            SmritiAction::Copy => "copy",
            SmritiAction::Reveal => "reveal",
            SmritiAction::Print => "print",
            SmritiAction::Enter => "enter",
        }
    }
}

/// One persisted Smriti path entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SmritiEntry {
    /// Absolute path recorded for this entry.
    pub path: String,
    /// File name derived from `path`, or the full path when no file name exists.
    pub name: String,
    /// Total accepted actions recorded for this path.
    pub total_count: u64,
    /// Number of editor-open actions recorded for this path.
    pub open_count: u64,
    /// Number of copy-path actions recorded for this path.
    pub copy_count: u64,
    /// Number of reveal-in-file-manager actions recorded for this path.
    pub reveal_count: u64,
    /// Number of print-path actions recorded for this path.
    pub print_count: u64,
    /// Number of enter-scope actions recorded for this path.
    pub enter_count: u64,
    /// Epoch seconds when this path was first recorded.
    pub first_used: i64,
    /// Epoch seconds when this path was most recently recorded.
    pub last_used: i64,
    /// Last query text associated with an accepted action for this path.
    pub last_query: String,
    /// Most recent action recorded for this path.
    pub last_action: SmritiAction,
}

impl SmritiEntry {
    fn new(path: String, query: String, action: SmritiAction, now: i64) -> Self {
        let name = Path::new(&path)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| path.clone());
        let mut entry = Self {
            path,
            name,
            total_count: 0,
            open_count: 0,
            copy_count: 0,
            reveal_count: 0,
            print_count: 0,
            enter_count: 0,
            first_used: now,
            last_used: now,
            last_query: String::new(),
            last_action: action,
        };
        entry.record(query, action, now);
        entry
    }

    fn record(&mut self, query: String, action: SmritiAction, now: i64) {
        self.total_count = self.total_count.saturating_add(1);
        match action {
            SmritiAction::Open => self.open_count = self.open_count.saturating_add(1),
            SmritiAction::Copy => self.copy_count = self.copy_count.saturating_add(1),
            SmritiAction::Reveal => self.reveal_count = self.reveal_count.saturating_add(1),
            SmritiAction::Print => self.print_count = self.print_count.saturating_add(1),
            SmritiAction::Enter => self.enter_count = self.enter_count.saturating_add(1),
        }
        self.last_used = now;
        self.last_query = query;
        self.last_action = action;
    }
}

/// Versioned on-disk Smriti document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SmritiStore {
    /// On-disk schema version.
    pub version: u16,
    /// Path-keyed usage memory entries.
    pub entries: HashMap<String, SmritiEntry>,
}

impl Default for SmritiStore {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            entries: HashMap::new(),
        }
    }
}

impl SmritiStore {
    /// Load a Smriti store from JSON, returning an empty store when the file is absent.
    ///
    /// JSON parse and filesystem errors are returned to the caller so the daemon can decide
    /// whether to warn and continue with empty memory.
    pub fn load(path: &Path) -> crate::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)?;
        let mut store: Self =
            serde_json::from_str(&content).map_err(|e| crate::Error::Config(e.to_string()))?;
        if store.version == 0 {
            store.version = CURRENT_VERSION;
        }
        Ok(store)
    }

    /// Persist the store as pretty JSON using a temporary file followed by `rename`.
    ///
    /// Parent directories are created as needed. Any write, serialization, or rename error is
    /// returned and the caller should treat the in-memory store as authoritative.
    pub fn save_atomic(&self, path: &Path) -> crate::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp_path = temp_path(path);
        let content =
            serde_json::to_vec_pretty(self).map_err(|e| crate::Error::Config(e.to_string()))?;
        std::fs::write(&tmp_path, content)?;
        std::fs::rename(tmp_path, path)?;
        Ok(())
    }

    /// Record one accepted action for `path`.
    ///
    /// Existing entries increment action counters and update `last_used`, `last_query`, and
    /// `last_action`; new entries derive their display name from the path.
    pub fn record(&mut self, path: String, query: String, action: SmritiAction, now: i64) {
        self.entries
            .entry(path.clone())
            .and_modify(|entry| entry.record(query.clone(), action, now))
            .or_insert_with(|| SmritiEntry::new(path, query, action, now));
    }

    /// Remove one path from usage memory.
    ///
    /// Returns `true` when an entry existed and was removed.
    pub fn forget(&mut self, path: &str) -> bool {
        self.entries.remove(path).is_some()
    }

    /// Remove all usage memory entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Keep only the most recently and frequently used entries up to `max_entries`.
    pub fn prune_to_limit(&mut self, max_entries: usize) {
        if self.entries.len() <= max_entries {
            return;
        }
        let mut ranked: Vec<(String, i64, u64)> = self
            .entries
            .iter()
            .map(|(path, entry)| (path.clone(), entry.last_used, entry.total_count))
            .collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.2.cmp(&a.2)));
        for (path, _, _) in ranked.into_iter().skip(max_entries) {
            self.entries.remove(&path);
        }
    }

    /// Return Smriti entries ordered by frecency.
    ///
    /// Optional query filtering matches path or file name substrings. Optional scope filtering
    /// keeps only entries whose paths start with that scope. The returned vector is truncated to
    /// `limit`.
    pub fn list(
        &self,
        query: Option<&str>,
        limit: usize,
        scope: Option<&Path>,
        now: i64,
    ) -> Vec<SmritiEntry> {
        let query = query.map(str::trim).filter(|q| !q.is_empty());
        let query_lower = query.map(str::to_lowercase);
        let mut entries: Vec<SmritiEntry> = self
            .entries
            .values()
            .filter(|entry| {
                if let Some(scope) = scope {
                    if !Path::new(&entry.path).starts_with(scope) {
                        return false;
                    }
                }
                if let Some(query) = query_lower.as_deref() {
                    let path = entry.path.to_lowercase();
                    let name = entry.name.to_lowercase();
                    return path.contains(query) || name.contains(query);
                }
                true
            })
            .cloned()
            .collect();
        entries.sort_by(|a, b| {
            frecency_score(b, now)
                .partial_cmp(&frecency_score(a, now))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.last_used.cmp(&a.last_used))
                .then_with(|| b.total_count.cmp(&a.total_count))
                .then_with(|| a.path.cmp(&b.path))
        });
        entries.truncate(limit);
        entries
    }

    /// Return the bounded ranking boost for a path at `now`.
    ///
    /// The result is clamped to `0.0..=max_boost`, and `max_boost` itself is sanitized to
    /// `0.0..=1.0` so malformed config cannot panic or overboost results.
    pub fn boost_for_path(&self, path: &str, now: i64, max_boost: f32) -> f32 {
        let Some(entry) = self.entries.get(path) else {
            return 0.0;
        };
        let max_boost = max_boost.clamp(0.0, 1.0);
        (frecency_score(entry, now) * max_boost).clamp(0.0, max_boost)
    }
}

fn temp_path(path: &Path) -> PathBuf {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    PathBuf::from(tmp)
}

fn frecency_score(entry: &SmritiEntry, now: i64) -> f32 {
    let age_secs = now.saturating_sub(entry.last_used).max(0) as f32;
    let age_days = age_secs / 86_400.0;
    let recency = 1.0 / (1.0 + (age_days / 7.0));
    let count = ((entry.total_count as f32 + 1.0).ln() / 50.0_f32.ln()).clamp(0.0, 1.0);
    ((recency * 0.7) + (count * 0.3)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_accumulates_counts_and_updates_last_action() {
        let mut store = SmritiStore::default();
        store.record(
            "/tmp/repo/src/main.rs".to_string(),
            "main".to_string(),
            SmritiAction::Open,
            100,
        );
        store.record(
            "/tmp/repo/src/main.rs".to_string(),
            "main".to_string(),
            SmritiAction::Copy,
            200,
        );

        let entry = store.entries.get("/tmp/repo/src/main.rs").unwrap();
        assert_eq!(entry.total_count, 2);
        assert_eq!(entry.open_count, 1);
        assert_eq!(entry.copy_count, 1);
        assert_eq!(entry.last_used, 200);
        assert_eq!(entry.last_action, SmritiAction::Copy);
    }

    #[test]
    fn boost_is_bounded_and_decays_with_age() {
        let mut store = SmritiStore::default();
        store.record(
            "/tmp/repo/src/main.rs".to_string(),
            "main".to_string(),
            SmritiAction::Open,
            1_000,
        );
        let fresh = store.boost_for_path("/tmp/repo/src/main.rs", 1_000, 0.08);
        let old = store.boost_for_path("/tmp/repo/src/main.rs", 1_000 + 86_400 * 60, 0.08);

        assert!(fresh > old);
        assert!(fresh <= 0.08);
        assert_eq!(store.boost_for_path("/tmp/missing", 1_000, 0.08), 0.0);
        assert_eq!(
            store.boost_for_path("/tmp/repo/src/main.rs", 1_000, -0.1),
            0.0
        );
        assert!(store.boost_for_path("/tmp/repo/src/main.rs", 1_000, 2.0) <= 1.0);
    }

    #[test]
    fn list_filters_by_query_and_scope() {
        let mut store = SmritiStore::default();
        store.record(
            "/tmp/repo/src/main.rs".to_string(),
            "main".to_string(),
            SmritiAction::Open,
            100,
        );
        store.record(
            "/tmp/other/README.md".to_string(),
            "readme".to_string(),
            SmritiAction::Open,
            200,
        );

        let scoped = store.list(Some("main"), 10, Some(Path::new("/tmp/repo")), 300);
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].name, "main.rs");
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("smriti.json");
        let mut store = SmritiStore::default();
        store.record(
            "/tmp/repo/Cargo.toml".to_string(),
            "cargo".to_string(),
            SmritiAction::Reveal,
            100,
        );
        store.save_atomic(&path).unwrap();

        let loaded = SmritiStore::load(&path).unwrap();
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(
            loaded.entries["/tmp/repo/Cargo.toml"].last_action,
            SmritiAction::Reveal
        );
    }
}
