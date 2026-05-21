//! Query engine for searching the index.

use crate::{AbbreviationMatcher, FileId, FileTable, StringArena, Trigram, TrigramIndex};
use serde::{Deserialize, Serialize};
use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;
use std::path::{Path, PathBuf};

const SHORT_QUERY_MAX_SCAN: usize = 50_000;
const SHORT_QUERY_MIN_SCAN_AFTER_LIMIT: usize = 10_000;
const INDEXED_QUERY_CANDIDATE_LIMIT: usize = 10_000;

/// A search query.
#[derive(Debug, Clone)]
pub struct Query {
    /// The search term (normalized).
    pub term: String,
    /// Maximum number of results.
    pub limit: usize,
    /// Optional scope root to boost (directory path).
    pub scope: Option<std::path::PathBuf>,
    /// Optional scope root used to strictly filter results to a subtree.
    pub filter_scope: Option<std::path::PathBuf>,
}

/// A search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// Full path to the file.
    pub path: String,
    /// Basename of the file.
    pub name: String,
    /// Score (0.0 to 1.0, higher is better).
    pub score: f32,
    /// File size.
    pub size: u64,
    /// Modification time.
    pub mtime: i64,
}

/// Query engine that searches the index.
pub struct QueryEngine<'a> {
    file_table: &'a FileTable,
    string_arena: &'a StringArena,
    trigram_index: &'a TrigramIndex,
}

#[derive(Debug, Clone, Copy)]
struct RankFeatures {
    context_score: i32,
    path_depth: usize,
}

struct QueryContext<'b> {
    boost_scope: Option<&'b Path>,
    filter_scope: Option<&'b Path>,
    cwd: Option<&'b Path>,
    abbr_matcher: AbbreviationMatcher,
}

impl<'a> QueryEngine<'a> {
    /// Create a new query engine.
    pub fn new(
        file_table: &'a FileTable,
        string_arena: &'a StringArena,
        trigram_index: &'a TrigramIndex,
    ) -> Self {
        Self {
            file_table,
            string_arena,
            trigram_index,
        }
    }

    /// Execute a search query.
    pub fn search(&self, query: &Query) -> Vec<SearchResult> {
        let normalized = query.term.to_lowercase();
        let cwd = std::env::current_dir().ok();
        let context = QueryContext {
            boost_scope: query.scope.as_deref(),
            filter_scope: query.filter_scope.as_deref(),
            cwd: cwd.as_deref(),
            abbr_matcher: AbbreviationMatcher::new(),
        };

        // For short queries, do a linear scan
        if normalized.len() < 3 {
            return self.linear_search(&normalized, query.limit, &context);
        }

        // Extract trigrams and query the index
        let trigrams = Trigram::extract(&normalized);
        let candidates = if let Some(filter_scope) = context.filter_scope {
            self.trigram_index.query_filtered_limited(
                &trigrams,
                INDEXED_QUERY_CANDIDATE_LIMIT,
                |file_id| {
                    let Some(meta) = self.file_table.get(file_id) else {
                        return false;
                    };
                    let Some(path) = self.string_arena.get(meta.path_offset, meta.path_len) else {
                        return false;
                    };
                    Self::scope_contains(Path::new(path), filter_scope, context.cwd)
                },
            )
        } else {
            self.trigram_index
                .query_limited(&trigrams, INDEXED_QUERY_CANDIDATE_LIMIT)
        };

        let mut ranked: Vec<(SearchResult, RankFeatures)> = Vec::with_capacity(query.limit);
        for file_id in candidates {
            if let Some(result) = self.score_candidate(file_id, &normalized, &context) {
                self.push_ranked_candidate(&mut ranked, result, query.limit);
            }
        }
        self.sort_ranked_results(&mut ranked);
        ranked.into_iter().map(|(r, _)| r).collect()
    }

    /// Execute a query against a pre-filtered set of file IDs.
    ///
    /// This is intended for daemon-side scope accelerators where enumerating a small
    /// subtree is cheaper than probing global posting lists and filtering afterward.
    pub fn search_file_ids(&self, query: &Query, file_ids: &[FileId]) -> Vec<SearchResult> {
        let normalized = query.term.to_lowercase();
        let cwd = std::env::current_dir().ok();
        let context = QueryContext {
            boost_scope: query.scope.as_deref(),
            filter_scope: query.filter_scope.as_deref(),
            cwd: cwd.as_deref(),
            abbr_matcher: AbbreviationMatcher::new(),
        };

        self.search_file_ids_normalized(&normalized, query.limit, file_ids, &context)
    }

    /// Score a candidate file.
    fn score_candidate(
        &self,
        file_id: FileId,
        query: &str,
        context: &QueryContext<'_>,
    ) -> Option<(SearchResult, RankFeatures)> {
        let meta = self.file_table.get(file_id)?;

        let path = self.string_arena.get(meta.path_offset, meta.path_len)?;
        let name = self.string_arena.get(meta.name_offset, meta.name_len)?;
        let path_buf = Path::new(path);

        if let Some(filter_scope) = context.filter_scope {
            if !Self::scope_contains(path_buf, filter_scope, context.cwd) {
                return None;
            }
        }

        let name_lower = lower_if_needed(name);
        let path_lower = lower_if_needed(path);

        // Try traditional substring matching
        let substring_score =
            if name_lower.as_ref().contains(query) || path_lower.as_ref().contains(query) {
                Some(self.calculate_score(name_lower.as_ref(), path_lower.as_ref(), query))
            } else {
                None
            };

        let abbr_score = if substring_score.is_some() || is_literal_filename_query(query) {
            None
        } else {
            context
                .abbr_matcher
                .match_path(query, path)
                .map(|abbr_match| abbr_match.score)
        };

        // Use the best score from either method
        let score = match (abbr_score, substring_score) {
            (Some(a), Some(s)) => a.max(s),
            (Some(a), None) => a,
            (None, Some(s)) => s,
            (None, None) => return None,
        };

        let path_depth = Self::path_depth(path);
        let features = RankFeatures {
            context_score: Self::context_score(path_lower.as_ref())
                + Self::scope_boost(path_buf, context.boost_scope, context.cwd),
            path_depth,
        };

        Some((
            SearchResult {
                path: path.to_string(),
                name: name.to_string(),
                score,
                size: meta.size,
                mtime: meta.mtime,
            },
            features,
        ))
    }

    /// Calculate match score (0.0 to 1.0).
    fn calculate_score(&self, name: &str, _path: &str, query: &str) -> f32 {
        // Exact match of entire basename (highest score)
        if name == query {
            return 1.0;
        }

        // Check for prefix match
        if name.starts_with(query) {
            // Prefer shorter suffixes - use ratio of query length to total length
            // This makes "main.rs" score higher than "main_test.rs"
            let ratio = query.len() as f32 / name.len() as f32;
            return 0.9 + (ratio * 0.09); // Range: 0.9 to 0.99
        }

        // Contains as whole word (after underscore or space)
        if name.contains(&format!(" {}", query)) || name.contains(&format!("_{}", query)) {
            return 0.7;
        }

        // Contains as substring
        if name.contains(query) {
            return 0.5;
        }

        // Default score for trigram matches
        0.3
    }

    /// Linear search for short queries.
    fn linear_search(
        &self,
        query: &str,
        limit: usize,
        context: &QueryContext<'_>,
    ) -> Vec<SearchResult> {
        if limit == 0 {
            return Vec::new();
        }

        let mut ranked: Vec<(SearchResult, RankFeatures)> = Vec::new();
        // Early termination: if we scan 1000 files without finding any matches,
        // assume the query won't match anything and stop (prevents hang on special chars)
        const MAX_EMPTY_SCAN: usize = 1000;

        for (scanned, (file_id, _meta)) in self.file_table.iter().enumerate() {
            // Early termination for non-matching queries
            if context.filter_scope.is_none() && ranked.is_empty() && scanned >= MAX_EMPTY_SCAN {
                break;
            }
            // One- and two-character queries are inherently broad over large home
            // indexes. Keep them interactive by sampling enough candidates to rank
            // useful results without letting a single keystroke monopolize daemon IPC.
            if scanned >= SHORT_QUERY_MAX_SCAN
                || (ranked.len() >= limit && scanned >= SHORT_QUERY_MIN_SCAN_AFTER_LIMIT)
            {
                break;
            }

            if let Some(result) = self.score_candidate(file_id, query, context) {
                self.push_ranked_candidate(&mut ranked, result, limit);
            }
        }

        self.sort_ranked_results(&mut ranked);
        ranked.into_iter().map(|(r, _)| r).collect()
    }

    fn search_file_ids_normalized(
        &self,
        query: &str,
        limit: usize,
        file_ids: &[FileId],
        context: &QueryContext<'_>,
    ) -> Vec<SearchResult> {
        if limit == 0 {
            return Vec::new();
        }

        let mut ranked: Vec<(SearchResult, RankFeatures)> = Vec::with_capacity(limit);
        for &file_id in file_ids {
            if let Some(result) = self.score_candidate(file_id, query, context) {
                self.push_ranked_candidate(&mut ranked, result, limit);
            }
        }

        self.sort_ranked_results(&mut ranked);
        ranked.into_iter().map(|(r, _)| r).collect()
    }

    fn sort_ranked_results(&self, ranked: &mut [(SearchResult, RankFeatures)]) {
        ranked.sort_by(Self::compare_ranked);
    }

    fn push_ranked_candidate(
        &self,
        ranked: &mut Vec<(SearchResult, RankFeatures)>,
        candidate: (SearchResult, RankFeatures),
        limit: usize,
    ) {
        if ranked.len() < limit {
            ranked.push(candidate);
            return;
        }

        if let Some((worst_index, worst)) = ranked
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| Self::compare_ranked(a, b))
        {
            if Self::compare_ranked(&candidate, worst) == Ordering::Less {
                ranked[worst_index] = candidate;
            }
        }
    }

    fn compare_ranked(
        (a, af): &(SearchResult, RankFeatures),
        (b, bf): &(SearchResult, RankFeatures),
    ) -> Ordering {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| bf.context_score.cmp(&af.context_score))
            .then_with(|| b.mtime.cmp(&a.mtime))
            .then_with(|| af.path_depth.cmp(&bf.path_depth))
            .then_with(|| a.path.cmp(&b.path))
    }

    fn path_depth(path: &str) -> usize {
        std::path::Path::new(path).components().count()
    }

    fn context_score(path_lower: &str) -> i32 {
        // Ranking-only penalties for common cache/build/tool-state directories.
        // These are intentionally conservative and only used as tie-breakers after
        // match score so that “the best textual match” still wins.
        //
        // Note: This is a first iteration; later phases make this configurable.
        let mut score = 0;

        // Dependency caches.
        if path_lower.contains("/go/pkg/mod/") {
            score -= 100;
        }
        if path_lower.contains("/node_modules/") {
            score -= 90;
        }
        if path_lower.contains("/.cargo/") {
            score -= 90;
        }
        if path_lower.contains("/.rustup/") {
            score -= 80;
        }
        if path_lower.contains("/.gradle/caches/") {
            score -= 80;
        }
        if path_lower.contains("/.m2/repository/") {
            score -= 80;
        }
        if path_lower.contains("/.nuget/packages/") {
            score -= 80;
        }
        if path_lower.contains("/site-packages/") {
            score -= 70;
        }
        if path_lower.contains("/.venv/") || path_lower.contains("/venv/") {
            score -= 70;
        }
        if path_lower.contains("/__pycache__/") {
            score -= 70;
        }

        // OS/application caches.
        if path_lower.contains("/library/caches/") || path_lower.contains("/.cache/") {
            score -= 80;
        }
        // Xcode build cache can be extremely noisy.
        if path_lower.contains("/library/developer/xcode/deriveddata/") {
            score -= 80;
        }

        // Build outputs / generated artifacts.
        if path_lower.contains("/target/")
            || path_lower.contains("/dist/")
            || path_lower.contains("/build/")
            || path_lower.contains("/out/")
        {
            score -= 60;
        }

        // Tool state.
        if path_lower.contains("/.git/") {
            score -= 40;
        }
        if path_lower.contains("/.idea/") || path_lower.contains("/.vscode/") {
            score -= 20;
        }

        score
    }

    fn scope_boost(path: &Path, scope: Option<&Path>, cwd: Option<&Path>) -> i32 {
        let Some(scope) = scope else {
            return 0;
        };
        let Some((path, scope)) = Self::scope_pair(path, scope, cwd) else {
            return 0;
        };

        // Scope boost should materially improve "search from within a repo"
        // without breaking cache demotions: it's additive on top of the
        // existing context penalties.
        //
        // Prefer closer (shallower) matches within the scope.
        let rel_depth = path
            .components()
            .count()
            .saturating_sub(scope.components().count());
        let depth_penalty = (rel_depth as i32).min(20);
        (120 - depth_penalty).max(0)
    }

    /// Returns the N most recently modified files, optionally filtered by scope.
    /// Used for populating TUI on startup when no query is provided.
    pub fn recent_files(&self, limit: usize, filter_scope: Option<&Path>) -> Vec<SearchResult> {
        if limit == 0 {
            return Vec::new();
        }

        let cwd = std::env::current_dir().ok();
        let cwd = cwd.as_deref();

        let mut heap: BinaryHeap<Reverse<RecentCandidate>> = BinaryHeap::with_capacity(limit + 1);

        for (file_id, meta) in self.file_table.iter() {
            let Some(path) = self.string_arena.get(meta.path_offset, meta.path_len) else {
                continue;
            };
            let Some(name) = self.string_arena.get(meta.name_offset, meta.name_len) else {
                continue;
            };

            // Skip tombstones and entries with empty names (e.g., root directories).
            if name.is_empty() {
                continue;
            }

            if let Some(scope_path) = filter_scope {
                if !Self::scope_contains(Path::new(path), scope_path, cwd) {
                    continue;
                }
            }

            heap.push(Reverse(RecentCandidate {
                mtime: meta.mtime,
                file_id,
            }));
            if heap.len() > limit {
                heap.pop();
            }
        }

        let mut candidates: Vec<RecentCandidate> = heap
            .into_iter()
            .map(|Reverse(candidate)| candidate)
            .collect();
        candidates.sort_by(|a, b| {
            b.mtime
                .cmp(&a.mtime)
                .then_with(|| a.file_id.cmp(&b.file_id))
        });

        candidates
            .into_iter()
            .filter_map(|candidate| {
                let meta = self.file_table.get(candidate.file_id)?;
                let path = self.string_arena.get(meta.path_offset, meta.path_len)?;
                let name = self.string_arena.get(meta.name_offset, meta.name_len)?;
                Some(SearchResult {
                    path: path.to_string(),
                    name: name.to_string(),
                    score: 0.0,
                    size: meta.size,
                    mtime: meta.mtime,
                })
            })
            .collect()
    }

    /// Returns the N most recently modified files from a pre-filtered set of file IDs.
    pub fn recent_file_ids(&self, limit: usize, file_ids: &[FileId]) -> Vec<SearchResult> {
        if limit == 0 {
            return Vec::new();
        }

        let mut heap: BinaryHeap<Reverse<RecentCandidate>> = BinaryHeap::with_capacity(limit + 1);

        for &file_id in file_ids {
            let Some(meta) = self.file_table.get(file_id) else {
                continue;
            };
            let Some(name) = self.string_arena.get(meta.name_offset, meta.name_len) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }

            heap.push(Reverse(RecentCandidate {
                mtime: meta.mtime,
                file_id,
            }));
            if heap.len() > limit {
                heap.pop();
            }
        }

        let mut candidates: Vec<RecentCandidate> = heap
            .into_iter()
            .map(|Reverse(candidate)| candidate)
            .collect();
        candidates.sort_by(|a, b| {
            b.mtime
                .cmp(&a.mtime)
                .then_with(|| a.file_id.cmp(&b.file_id))
        });

        candidates
            .into_iter()
            .filter_map(|candidate| {
                let meta = self.file_table.get(candidate.file_id)?;
                let path = self.string_arena.get(meta.path_offset, meta.path_len)?;
                let name = self.string_arena.get(meta.name_offset, meta.name_len)?;
                Some(SearchResult {
                    path: path.to_string(),
                    name: name.to_string(),
                    score: 0.0,
                    size: meta.size,
                    mtime: meta.mtime,
                })
            })
            .collect()
    }

    /// Returns exact-basename matches from a daemon-maintained name index.
    pub fn exact_name_file_ids(&self, limit: usize, file_ids: &[FileId]) -> Vec<SearchResult> {
        let mut results: Vec<SearchResult> = file_ids
            .iter()
            .filter_map(|&file_id| {
                let meta = self.file_table.get(file_id)?;
                let path = self.string_arena.get(meta.path_offset, meta.path_len)?;
                let name = self.string_arena.get(meta.name_offset, meta.name_len)?;
                if name.is_empty() {
                    return None;
                }
                Some(SearchResult {
                    path: path.to_string(),
                    name: name.to_string(),
                    score: 1.0,
                    size: meta.size,
                    mtime: meta.mtime,
                })
            })
            .collect();

        results.sort_by(|a, b| {
            b.mtime
                .cmp(&a.mtime)
                .then_with(|| path_depth_str(&a.path).cmp(&path_depth_str(&b.path)))
                .then_with(|| a.path.cmp(&b.path))
        });
        results.truncate(limit);
        results
    }

    fn scope_contains(path: &Path, scope: &Path, cwd: Option<&Path>) -> bool {
        Self::scope_pair(path, scope, cwd).is_some()
    }

    fn scope_pair(path: &Path, scope: &Path, cwd: Option<&Path>) -> Option<(PathBuf, PathBuf)> {
        if path.starts_with(scope) {
            return Some((path.to_path_buf(), scope.to_path_buf()));
        }

        let normalized_path = Self::normalize_scope_path(path, cwd)?;
        let normalized_scope = Self::normalize_scope_path(scope, cwd)?;
        normalized_path
            .starts_with(&normalized_scope)
            .then_some((normalized_path, normalized_scope))
    }

    fn normalize_scope_path(path: &Path, cwd: Option<&Path>) -> Option<PathBuf> {
        if path.is_absolute() {
            return Some(Self::normalize_absolute_path(path));
        }

        cwd.map(|cwd| Self::normalize_absolute_path(&cwd.join(path)))
    }

    fn normalize_absolute_path(path: &Path) -> PathBuf {
        use std::path::Component;

        let mut normalized = PathBuf::new();
        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    normalized.pop();
                }
                Component::RootDir | Component::Prefix(_) | Component::Normal(_) => {
                    normalized.push(component.as_os_str());
                }
            }
        }
        normalized
    }
}

fn lower_if_needed(text: &str) -> std::borrow::Cow<'_, str> {
    if text.is_ascii() {
        if text.bytes().any(|b| b.is_ascii_uppercase()) {
            std::borrow::Cow::Owned(text.to_lowercase())
        } else {
            std::borrow::Cow::Borrowed(text)
        }
    } else if text.chars().any(|c| c.is_uppercase()) {
        std::borrow::Cow::Owned(text.to_lowercase())
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

fn is_literal_filename_query(query: &str) -> bool {
    query
        .bytes()
        .any(|b| matches!(b, b'.' | b'-' | b'_' | b'/' | b'\\'))
}

fn path_depth_str(path: &str) -> usize {
    std::path::Path::new(path).components().count()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RecentCandidate {
    mtime: i64,
    file_id: FileId,
}

impl Ord for RecentCandidate {
    fn cmp(&self, other: &Self) -> Ordering {
        self.mtime
            .cmp(&other.mtime)
            .then_with(|| other.file_id.cmp(&self.file_id))
    }
}

impl PartialOrd for RecentCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FileMeta;

    #[test]
    fn test_query_engine() {
        let mut file_table = FileTable::new();
        let mut arena = StringArena::new();
        let mut index = TrigramIndex::new();

        // Add some test files
        let (path_off, path_len) = arena.add("/home/user/test.txt");
        let (name_off, name_len) = arena.add("test.txt");

        let meta = FileMeta {
            path_offset: path_off,
            path_len,
            name_offset: name_off,
            name_len,
            size: 1024,
            mtime: 0,
            dev: 0,
            ino: 0,
        };

        let file_id = file_table.insert(meta);
        index.add(file_id, "test.txt");

        let engine = QueryEngine::new(&file_table, &arena, &index);
        let query = Query {
            term: "test".to_string(),
            limit: 10,
            scope: None,
            filter_scope: None,
        };

        let results = engine.search(&query);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "test.txt");
    }

    #[test]
    fn unicode_uppercase_filename_matches_lowercase_query() {
        let mut file_table = FileTable::new();
        let mut arena = StringArena::new();
        let mut index = TrigramIndex::new();

        let (path_off, path_len) = arena.add("/repo/Überblick.md");
        let (name_off, name_len) = arena.add("Überblick.md");
        let file_id = file_table.insert(FileMeta {
            path_offset: path_off,
            path_len,
            name_offset: name_off,
            name_len,
            size: 1,
            mtime: 0,
            dev: 0,
            ino: 0,
        });
        index.add(file_id, "Überblick.md");

        let engine = QueryEngine::new(&file_table, &arena, &index);
        let results = engine.search(&Query {
            term: "über".to_string(),
            limit: 10,
            scope: None,
            filter_scope: None,
        });

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Überblick.md");
    }

    #[test]
    fn test_early_termination_for_non_matching() {
        let mut file_table = FileTable::new();
        let mut arena = StringArena::new();
        let mut index = TrigramIndex::new();

        // Add 2000 test files that don't contain special characters
        for i in 0..2000 {
            let path = format!("/home/user/file_{}.txt", i);
            let name = format!("file_{}.txt", i);

            let (path_off, path_len) = arena.add(&path);
            let (name_off, name_len) = arena.add(&name);

            let meta = FileMeta {
                path_offset: path_off,
                path_len,
                name_offset: name_off,
                name_len,
                size: 1024,
                mtime: 0,
                dev: 0,
                ino: 0,
            };

            let file_id = file_table.insert(meta);
            index.add(file_id, &name);
        }

        let engine = QueryEngine::new(&file_table, &arena, &index);

        // Query with special char that won't match any files
        let query = Query {
            term: "*".to_string(),
            limit: 100,
            scope: None,
            filter_scope: None,
        };

        let start = std::time::Instant::now();
        let results = engine.search(&query);
        let elapsed = start.elapsed();

        // Should find 0 results
        assert_eq!(results.len(), 0);

        // Should complete in < 50ms due to early termination (not scan all 2000 files)
        // With MAX_EMPTY_SCAN=1000, should scan ~1000 files in debug mode
        // Note: Generous threshold to handle CI variability; release builds are ~3-5ms
        assert!(
            elapsed.as_millis() < 50,
            "Search took {:?}, expected < 50ms due to early termination",
            elapsed
        );
    }

    #[test]
    fn test_no_regression_for_matching_queries() {
        let mut file_table = FileTable::new();
        let mut arena = StringArena::new();
        let mut index = TrigramIndex::new();

        // Add files, some with digit "5"
        for i in 0..1500 {
            let path = format!("/home/user/file_{}.txt", i);
            let name = format!("file_{}.txt", i);

            let (path_off, path_len) = arena.add(&path);
            let (name_off, name_len) = arena.add(&name);

            let meta = FileMeta {
                path_offset: path_off,
                path_len,
                name_offset: name_off,
                name_len,
                size: 1024,
                mtime: 0,
                dev: 0,
                ino: 0,
            };

            let file_id = file_table.insert(meta);
            index.add(file_id, &name);
        }

        let engine = QueryEngine::new(&file_table, &arena, &index);

        // Query for "5" which appears in many files
        let query = Query {
            term: "5".to_string(),
            limit: 50,
            scope: None,
            filter_scope: None,
        };

        let results = engine.search(&query);

        // Should find exactly 50 results (the limit)
        assert_eq!(results.len(), 50);

        // All results should contain "5"
        for result in results {
            assert!(
                result.name.contains('5'),
                "Result {} doesn't contain '5'",
                result.name
            );
        }
    }

    #[test]
    fn test_short_queries_rank_best_matches_not_first_matches() {
        let mut file_table = FileTable::new();
        let mut arena = StringArena::new();
        let mut index = TrigramIndex::new();

        for name in [
            "base.txt",
            "case-study.txt",
            "search.rs",
            "server.rs",
            "session.rs",
        ] {
            let path = format!("/repo/{name}");
            let (path_off, path_len) = arena.add(&path);
            let (name_off, name_len) = arena.add(name);

            let meta = FileMeta {
                path_offset: path_off,
                path_len,
                name_offset: name_off,
                name_len,
                size: 1024,
                mtime: 0,
                dev: 0,
                ino: 0,
            };

            let file_id = file_table.insert(meta);
            index.add(file_id, name);
        }

        let engine = QueryEngine::new(&file_table, &arena, &index);
        let query = Query {
            term: "se".to_string(),
            limit: 2,
            scope: None,
            filter_scope: None,
        };

        let results = engine.search(&query);
        let names: Vec<_> = results.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["search.rs", "server.rs"]);
    }

    #[test]
    fn test_common_indexed_queries_are_candidate_capped() {
        let mut file_table = FileTable::new();
        let mut arena = StringArena::new();
        let mut index = TrigramIndex::new();

        for i in 0..15_000 {
            let path = format!("/home/user/site-packages/pkg_{i}/RECORD");
            let name = "RECORD";
            let (path_off, path_len) = arena.add(&path);
            let (name_off, name_len) = arena.add(name);
            let meta = FileMeta {
                path_offset: path_off,
                path_len,
                name_offset: name_off,
                name_len,
                size: 1024,
                mtime: i,
                dev: 0,
                ino: i as u64,
            };

            let file_id = file_table.insert(meta);
            index.add(file_id, name);
        }

        let engine = QueryEngine::new(&file_table, &arena, &index);
        let results = engine.search(&Query {
            term: "record".to_string(),
            limit: 10,
            scope: Some(PathBuf::from("/home/user")),
            filter_scope: None,
        });

        assert_eq!(results.len(), 10);
        assert!(results.iter().all(|result| result.name == "RECORD"));
    }

    #[test]
    fn test_scoped_indexed_search_filters_before_effective_limit() {
        let mut file_table = FileTable::new();
        let mut arena = StringArena::new();
        let mut index = TrigramIndex::new();

        for i in 0..(INDEXED_QUERY_CANDIDATE_LIMIT + 10) {
            let path = format!("/outside/site-packages/pkg_{i}/RECORD");
            let name = "RECORD";
            let (path_off, path_len) = arena.add(&path);
            let (name_off, name_len) = arena.add(name);
            let meta = FileMeta {
                path_offset: path_off,
                path_len,
                name_offset: name_off,
                name_len,
                size: 1024,
                mtime: i as i64,
                dev: 0,
                ino: i as u64,
            };

            let file_id = file_table.insert(meta);
            index.add(file_id, name);
        }

        let inside_path = "/inside/notes/recording.md";
        let (path_off, path_len) = arena.add(inside_path);
        let (name_off, name_len) = arena.add("recording.md");
        let file_id = file_table.insert(FileMeta {
            path_offset: path_off,
            path_len,
            name_offset: name_off,
            name_len,
            size: 512,
            mtime: 99_999,
            dev: 0,
            ino: 99_999,
        });
        index.add(file_id, "recording.md");

        let engine = QueryEngine::new(&file_table, &arena, &index);
        let results = engine.search(&Query {
            term: "record".to_string(),
            limit: 10,
            scope: Some(PathBuf::from("/inside")),
            filter_scope: Some(PathBuf::from("/inside")),
        });

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, inside_path);
    }

    #[test]
    fn test_recent_files_excludes_empty_names() {
        let mut file_table = FileTable::new();
        let mut arena = StringArena::new();
        let index = TrigramIndex::new();

        // Add a normal file
        let (path_off1, path_len1) = arena.add("/home/user/test.txt");
        let (name_off1, name_len1) = arena.add("test.txt");
        let meta1 = FileMeta {
            path_offset: path_off1,
            path_len: path_len1,
            name_offset: name_off1,
            name_len: name_len1,
            size: 1024,
            mtime: 100,
            dev: 0,
            ino: 0,
        };
        file_table.insert(meta1);

        // Add an entry with an empty name (simulating a root directory or corrupted entry)
        let (path_off2, path_len2) = arena.add("/");
        let (name_off2, name_len2) = arena.add("");
        let meta2 = FileMeta {
            path_offset: path_off2,
            path_len: path_len2,
            name_offset: name_off2,
            name_len: name_len2,
            size: 0,
            mtime: 200, // More recent mtime
            dev: 0,
            ino: 0,
        };
        file_table.insert(meta2);

        // Add another normal file
        let (path_off3, path_len3) = arena.add("/home/user/other.rs");
        let (name_off3, name_len3) = arena.add("other.rs");
        let meta3 = FileMeta {
            path_offset: path_off3,
            path_len: path_len3,
            name_offset: name_off3,
            name_len: name_len3,
            size: 2048,
            mtime: 50,
            dev: 0,
            ino: 0,
        };
        file_table.insert(meta3);

        let engine = QueryEngine::new(&file_table, &arena, &index);
        let results = engine.recent_files(10, None);

        // Should only have 2 results (the empty-name entry is filtered out)
        assert_eq!(results.len(), 2);

        // Results should be sorted by mtime desc
        assert_eq!(results[0].name, "test.txt"); // mtime=100
        assert_eq!(results[1].name, "other.rs"); // mtime=50

        // Verify no empty names
        for result in &results {
            assert!(!result.name.is_empty(), "Found empty name in results");
        }
    }

    #[test]
    fn test_filter_scope_excludes_out_of_scope_exact_matches() {
        let mut file_table = FileTable::new();
        let mut arena = StringArena::new();
        let mut index = TrigramIndex::new();

        for path in ["/repo-a/query.rs", "/repo-b/query.rs"] {
            let name = "query.rs";
            let (path_off, path_len) = arena.add(path);
            let (name_off, name_len) = arena.add(name);
            let meta = FileMeta {
                path_offset: path_off,
                path_len,
                name_offset: name_off,
                name_len,
                size: 128,
                mtime: 0,
                dev: 0,
                ino: 0,
            };
            let file_id = file_table.insert(meta);
            index.add(file_id, name);
        }

        let engine = QueryEngine::new(&file_table, &arena, &index);
        let query = Query {
            term: "query.rs".to_string(),
            limit: 10,
            scope: Some(std::path::PathBuf::from("/repo-a")),
            filter_scope: Some(std::path::PathBuf::from("/repo-a")),
        };

        let results = engine.search(&query);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "/repo-a/query.rs");
    }

    #[test]
    fn test_filter_scope_matches_relative_indexed_paths_against_absolute_scope() {
        use std::sync::{Mutex, OnceLock};

        static CWD_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let _cwd_lock = CWD_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();

        let mut file_table = FileTable::new();
        let mut arena = StringArena::new();
        let mut index = TrigramIndex::new();

        for path in ["workspace/repo-a/query.rs", "workspace/repo-b/query.rs"] {
            let name = "query.rs";
            let (path_off, path_len) = arena.add(path);
            let (name_off, name_len) = arena.add(name);
            let meta = FileMeta {
                path_offset: path_off,
                path_len,
                name_offset: name_off,
                name_len,
                size: 128,
                mtime: 0,
                dev: 0,
                ino: 0,
            };
            let file_id = file_table.insert(meta);
            index.add(file_id, name);
        }

        let tempdir = std::env::temp_dir().join(format!(
            "vicaya-index-scope-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tempdir).unwrap();
        let original_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tempdir).unwrap();
        let resolved_cwd = std::env::current_dir().unwrap();

        let engine = QueryEngine::new(&file_table, &arena, &index);
        let query = Query {
            term: "query.rs".to_string(),
            limit: 10,
            scope: Some(resolved_cwd.join("workspace/repo-a")),
            filter_scope: Some(resolved_cwd.join("workspace/repo-a")),
        };

        let results = engine.search(&query);
        std::env::set_current_dir(original_cwd).unwrap();
        std::fs::remove_dir_all(&tempdir).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "workspace/repo-a/query.rs");
    }

    #[test]
    fn test_short_scoped_queries_scan_past_empty_global_prefix() {
        let mut file_table = FileTable::new();
        let mut arena = StringArena::new();
        let mut index = TrigramIndex::new();

        for i in 0..1200 {
            let path = format!("/outside/outside_{i}.txt");
            let name = format!("outside_{i}.txt");
            let (path_off, path_len) = arena.add(&path);
            let (name_off, name_len) = arena.add(&name);
            let meta = FileMeta {
                path_offset: path_off,
                path_len,
                name_offset: name_off,
                name_len,
                size: 1,
                mtime: 0,
                dev: 0,
                ino: 0,
            };
            let file_id = file_table.insert(meta);
            index.add(file_id, &name);
        }

        let (path_off, path_len) = arena.add("/repo-a/src/qa.rs");
        let (name_off, name_len) = arena.add("qa.rs");
        let file_id = file_table.insert(FileMeta {
            path_offset: path_off,
            path_len,
            name_offset: name_off,
            name_len,
            size: 1,
            mtime: 1,
            dev: 0,
            ino: 0,
        });
        index.add(file_id, "qa.rs");

        let engine = QueryEngine::new(&file_table, &arena, &index);
        let query = Query {
            term: "qa".to_string(),
            limit: 10,
            scope: Some(PathBuf::from("/repo-a")),
            filter_scope: Some(PathBuf::from("/repo-a")),
        };

        let results = engine.search(&query);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "/repo-a/src/qa.rs");
    }

    #[test]
    fn test_recent_files_respects_filter_scope() {
        let mut file_table = FileTable::new();
        let mut arena = StringArena::new();
        let index = TrigramIndex::new();

        for (path, name, mtime) in [
            ("/repo-a/new.rs", "new.rs", 300),
            ("/repo-b/other.rs", "other.rs", 200),
            ("/repo-a/older.rs", "older.rs", 100),
        ] {
            let (path_off, path_len) = arena.add(path);
            let (name_off, name_len) = arena.add(name);
            let meta = FileMeta {
                path_offset: path_off,
                path_len,
                name_offset: name_off,
                name_len,
                size: 1,
                mtime,
                dev: 0,
                ino: 0,
            };
            file_table.insert(meta);
        }

        let engine = QueryEngine::new(&file_table, &arena, &index);
        let results = engine.recent_files(10, Some(std::path::Path::new("/repo-a")));

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].path, "/repo-a/new.rs");
        assert_eq!(results[1].path, "/repo-a/older.rs");
    }
}
