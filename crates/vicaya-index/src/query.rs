//! Query engine for searching the index.

use crate::{AbbreviationMatcher, FileId, FileTable, StringArena, Trigram, TrigramIndex};
use serde::{Deserialize, Serialize};

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
        let mut results: Vec<_> = candidates
            .iter()
            .filter_map(|&file_id| self.score_candidate(file_id, &normalized))
            .collect();

        // Sort by score (descending)
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        // Limit results
        results.truncate(query.limit);
        results
    }

    /// Score a candidate file.
    fn score_candidate(&self, file_id: FileId, query: &str) -> Option<SearchResult> {
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

        Some(SearchResult {
            path: path.to_string(),
            name: name.to_string(),
            score,
            size: meta.size,
            mtime: meta.mtime,
        })
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
        let mut results = Vec::new();

        for (file_id, _meta) in self.file_table.iter() {
            if results.len() >= limit {
                break;
            }

            if let Some(result) = self.score_candidate(file_id, query) {
                results.push(result);
            }
        }

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        results
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
}
