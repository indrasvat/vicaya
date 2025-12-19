//! Query engine for searching the index.

use crate::{AbbreviationMatcher, FileId, FileTable, StringArena, Trigram, TrigramIndex};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// A search query.
#[derive(Debug, Clone)]
pub struct Query {
    /// The search term (normalized).
    pub term: String,
    /// Maximum number of results.
    pub limit: usize,
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

        // For short queries, do a linear scan
        if normalized.len() < 3 {
            return self.linear_search(&normalized, query.limit);
        }

        // Extract trigrams and query the index
        let trigrams = Trigram::extract(&normalized);
        let candidates = self.trigram_index.query(&trigrams);

        // Score and filter candidates
        let mut ranked: Vec<(SearchResult, RankFeatures)> = candidates
            .iter()
            .filter_map(|&file_id| self.score_candidate(file_id, &normalized))
            .collect();

        self.sort_ranked_results(&mut ranked);

        // Limit results
        ranked.truncate(query.limit);
        ranked.into_iter().map(|(r, _)| r).collect()
    }

    /// Score a candidate file.
    fn score_candidate(&self, file_id: FileId, query: &str) -> Option<(SearchResult, RankFeatures)> {
        let meta = self.file_table.get(file_id)?;

        let path = self.string_arena.get(meta.path_offset, meta.path_len)?;
        let name = self.string_arena.get(meta.name_offset, meta.name_len)?;

        // Check if the query matches the basename or path
        let name_lower = name.to_lowercase();
        let path_lower = path.to_lowercase();

        // Try abbreviation matching first (especially for short queries)
        let abbr_matcher = AbbreviationMatcher::new();
        let abbr_score = if let Some(abbr_match) = abbr_matcher.match_path(query, path) {
            Some(abbr_match.score)
        } else {
            None
        };

        // Try traditional substring matching
        let substring_score = if name_lower.contains(query) || path_lower.contains(query) {
            Some(self.calculate_score(&name_lower, &path_lower, query))
        } else {
            None
        };

        // Use the best score from either method
        let score = match (abbr_score, substring_score) {
            (Some(a), Some(s)) => a.max(s),
            (Some(a), None) => a,
            (None, Some(s)) => s,
            (None, None) => return None,
        };

        let features = RankFeatures {
            context_score: Self::context_score(&path_lower),
            path_depth: Self::path_depth(path),
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
    fn linear_search(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        let mut ranked: Vec<(SearchResult, RankFeatures)> = Vec::new();
        // Early termination: if we scan 1000 files without finding any matches,
        // assume the query won't match anything and stop (prevents hang on special chars)
        const MAX_EMPTY_SCAN: usize = 1000;

        for (scanned, (file_id, _meta)) in self.file_table.iter().enumerate() {
            if ranked.len() >= limit {
                break;
            }

            // Early termination for non-matching queries
            if ranked.is_empty() && scanned >= MAX_EMPTY_SCAN {
                break;
            }

            if let Some(result) = self.score_candidate(file_id, query) {
                ranked.push(result);
            }
        }

        self.sort_ranked_results(&mut ranked);
        ranked.into_iter().map(|(r, _)| r).collect()
    }

    fn sort_ranked_results(&self, ranked: &mut [(SearchResult, RankFeatures)]) {
        ranked.sort_by(|(a, af), (b, bf)| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| bf.context_score.cmp(&af.context_score))
                .then_with(|| b.mtime.cmp(&a.mtime))
                .then_with(|| af.path_depth.cmp(&bf.path_depth))
                .then_with(|| a.path.cmp(&b.path))
        });
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
        };

        let results = engine.search(&query);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "test.txt");
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
}
