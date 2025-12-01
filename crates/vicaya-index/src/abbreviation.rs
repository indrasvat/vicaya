//! Smart abbreviation matching for file paths.
//!
//! This module implements multiple strategies for matching query abbreviations
//! against file paths, enabling users to find files with minimal typing.
//!
//! # Matching Strategies
//!
//! 1. **Exact Prefix**: Query exactly matches the start of a path component
//!    - `"main"` matches `"src/main.rs"` (score: 1.0)
//!
//! 2. **Component First-Letter**: Query matches first letters of path components
//!    - `"vcs"` matches `"vicaya-core/src/main.rs"` (score: ~0.95)
//!
//! 3. **CamelCase**: Query matches uppercase letters or word boundaries
//!    - `"CT"` matches `"Cargo.toml"` (score: ~0.90)
//!
//! 4. **Sequential**: Query characters appear in order with gaps
//!    - `"main"` matches `"admin/main.rs"` (score: ~0.70-0.85)

use std::path::Path;

/// Result of an abbreviation match.
#[derive(Debug, Clone, PartialEq)]
pub struct AbbreviationMatch {
    /// Match score from 0.0 to 1.0 (higher is better)
    pub score: f32,
    /// Which strategy produced this match
    pub strategy: MatchStrategy,
    /// Indices in the path where query characters matched (for highlighting)
    pub matched_indices: Vec<usize>,
}

/// Strategy used to match the abbreviation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchStrategy {
    /// Exact match at component start
    ExactPrefix,
    /// First letters of path components
    ComponentFirst,
    /// CamelCase or word boundary matching
    CamelCase,
    /// Sequential character matching with gaps
    Sequential,
}

/// Matcher for abbreviation-style queries.
#[derive(Debug, Default)]
pub struct AbbreviationMatcher {
    /// Whether to perform case-sensitive matching
    case_sensitive: bool,
}

impl AbbreviationMatcher {
    /// Create a new abbreviation matcher with default settings.
    pub fn new() -> Self {
        Self {
            case_sensitive: false,
        }
    }

    /// Create a case-sensitive matcher.
    pub fn case_sensitive() -> Self {
        Self {
            case_sensitive: true,
        }
    }

    /// Try to match query as an abbreviation against the given path.
    ///
    /// Returns the best match found across all strategies, or None if
    /// no strategy could match.
    pub fn match_path(&self, query: &str, path: &str) -> Option<AbbreviationMatch> {
        if query.is_empty() {
            return None;
        }

        // Normalize inputs for matching
        let query_lower = if self.case_sensitive {
            query.to_string()
        } else {
            query.to_lowercase()
        };

        // Try all strategies and pick the best match
        let mut best_match: Option<AbbreviationMatch> = None;

        // Strategy 1: Exact prefix (highest score)
        if let Some(m) = self.match_exact_prefix(&query_lower, path) {
            best_match = Some(m);
        }

        // Strategy 2: Component first-letter
        if let Some(m) = self.match_component_first(&query_lower, path) {
            if best_match.as_ref().is_none_or(|bm| m.score > bm.score) {
                best_match = Some(m);
            }
        }

        // Strategy 3: CamelCase
        if let Some(m) = self.match_camelcase(&query_lower, query, path) {
            if best_match.as_ref().is_none_or(|bm| m.score > bm.score) {
                best_match = Some(m);
            }
        }

        // Strategy 4: Sequential (fallback)
        if let Some(m) = self.match_sequential(&query_lower, path) {
            if best_match.as_ref().is_none_or(|bm| m.score > bm.score) {
                best_match = Some(m);
            }
        }

        best_match
    }

    /// Match exact prefix of a path component.
    ///
    /// Example: "main" matches "src/main.rs"
    fn match_exact_prefix(&self, query: &str, path: &str) -> Option<AbbreviationMatch> {
        let path_lower = if self.case_sensitive {
            path.to_string()
        } else {
            path.to_lowercase()
        };

        // Extract filename from path
        let filename = Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(path);

        let filename_lower = if self.case_sensitive {
            filename.to_string()
        } else {
            filename.to_lowercase()
        };

        // Check if query is exact prefix of filename (ignoring extension)
        let stem = Path::new(&filename_lower)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&filename_lower);

        if stem.starts_with(query) {
            let matched_indices: Vec<usize> = (0..query.len()).collect();
            // Give perfect score for exact match, slightly lower for prefix
            let score = if stem == query { 1.0 } else { 0.99 };
            return Some(AbbreviationMatch {
                score,
                strategy: MatchStrategy::ExactPrefix,
                matched_indices,
            });
        }

        // Also check each path component
        for component in Path::new(&path_lower).components() {
            if let Some(comp_str) = component.as_os_str().to_str() {
                if comp_str.starts_with(query) {
                    let matched_indices: Vec<usize> = (0..query.len()).collect();
                    return Some(AbbreviationMatch {
                        score: 0.98, // Slightly lower than filename prefix
                        strategy: MatchStrategy::ExactPrefix,
                        matched_indices,
                    });
                }
            }
        }

        None
    }

    /// Match first letters of path components.
    ///
    /// Example: "vcs" matches "vicaya-core/src/main.rs"
    fn match_component_first(&self, query: &str, path: &str) -> Option<AbbreviationMatch> {
        let components = Self::tokenize_path(path);
        if components.is_empty() {
            return None;
        }

        // Extract first letters from each token
        let first_letters: Vec<char> = components
            .iter()
            .filter_map(|token| token.chars().next())
            .map(|c| {
                if self.case_sensitive {
                    c
                } else {
                    c.to_lowercase().next().unwrap_or(c)
                }
            })
            .collect();

        // Try to match query characters to first letters sequentially
        let query_chars: Vec<char> = query.chars().collect();
        let mut matched_indices = Vec::new();
        let mut query_idx = 0;
        let mut component_idx = 0;

        while query_idx < query_chars.len() && component_idx < first_letters.len() {
            if query_chars[query_idx] == first_letters[component_idx] {
                matched_indices.push(component_idx);
                query_idx += 1;
            }
            component_idx += 1;
        }

        // Check if we matched all query characters
        if query_idx == query_chars.len() {
            // Calculate score based on match quality
            let base_score = 0.95;
            let consecutive_bonus = self.calculate_consecutive_bonus(&matched_indices, query.len());
            let coverage_ratio = matched_indices.len() as f32 / first_letters.len() as f32;
            let coverage_bonus = coverage_ratio * 0.05;

            let score = (base_score + consecutive_bonus + coverage_bonus).min(0.99);

            return Some(AbbreviationMatch {
                score,
                strategy: MatchStrategy::ComponentFirst,
                matched_indices,
            });
        }

        None
    }

    /// Match CamelCase or word boundary positions.
    ///
    /// Example: "CT" matches "Cargo.toml" or "cargo_test"
    fn match_camelcase(
        &self,
        query_lower: &str,
        query_original: &str,
        path: &str,
    ) -> Option<AbbreviationMatch> {
        // Extract positions where capitals or word boundaries occur
        let capital_positions: Vec<(usize, char)> = path
            .char_indices()
            .filter(|(i, c)| {
                // Include: uppercase letters, chars after separators
                c.is_uppercase()
                    || (*i > 0
                        && path.chars().nth(i - 1).is_some_and(|prev| {
                            prev == '_' || prev == '-' || prev == '/' || prev == '.'
                        }))
            })
            .map(|(i, c)| {
                (
                    i,
                    if self.case_sensitive {
                        c
                    } else {
                        c.to_lowercase().next().unwrap_or(c)
                    },
                )
            })
            .collect();

        if capital_positions.is_empty() {
            return None;
        }

        // Try to match query characters to capital positions
        let query_chars: Vec<char> = query_lower.chars().collect();
        let mut matched_indices = Vec::new();
        let mut query_idx = 0;
        let mut cap_idx = 0;

        while query_idx < query_chars.len() && cap_idx < capital_positions.len() {
            if query_chars[query_idx] == capital_positions[cap_idx].1 {
                matched_indices.push(capital_positions[cap_idx].0);
                query_idx += 1;
            }
            cap_idx += 1;
        }

        // Check if we matched all query characters
        if query_idx == query_chars.len() {
            let base_score = 0.90;
            let consecutive_bonus =
                self.calculate_consecutive_bonus(&matched_indices, query_lower.len());

            // Bonus for matching actual uppercase in query to uppercase in path
            let case_match_bonus =
                if query_original
                    .chars()
                    .zip(matched_indices.iter())
                    .all(|(qc, &idx)| {
                        qc.is_uppercase()
                            && path.chars().nth(idx).is_some_and(|pc| pc.is_uppercase())
                    })
                {
                    0.05
                } else {
                    0.0
                };

            let score = (base_score + consecutive_bonus + case_match_bonus).min(0.96);

            return Some(AbbreviationMatch {
                score,
                strategy: MatchStrategy::CamelCase,
                matched_indices,
            });
        }

        None
    }

    /// Match query characters sequentially with gaps allowed.
    ///
    /// Example: "main" matches "admin/main.rs"
    fn match_sequential(&self, query: &str, path: &str) -> Option<AbbreviationMatch> {
        let path_lower = if self.case_sensitive {
            path.to_string()
        } else {
            path.to_lowercase()
        };

        let query_chars: Vec<char> = query.chars().collect();
        let path_chars: Vec<char> = path_lower.chars().collect();

        let mut matched_indices = Vec::new();
        let mut query_idx = 0;
        let mut path_idx = 0;

        // Try to match all query characters in order
        while query_idx < query_chars.len() && path_idx < path_chars.len() {
            if query_chars[query_idx] == path_chars[path_idx] {
                matched_indices.push(path_idx);
                query_idx += 1;
            }
            path_idx += 1;
        }

        // Check if we matched all query characters
        if query_idx == query_chars.len() {
            // Calculate score based on match quality
            let base_score = 0.70;

            // Bonus for consecutive matches
            let consecutive_bonus = self.calculate_consecutive_bonus(&matched_indices, query.len());

            // Bonus for matches closer to end (filename)
            let avg_pos =
                matched_indices.iter().sum::<usize>() as f32 / matched_indices.len() as f32;
            let position_ratio = avg_pos / path_chars.len() as f32;
            let position_bonus = position_ratio * 0.10;

            // Penalty for large gaps
            let total_span = matched_indices.last().unwrap() - matched_indices.first().unwrap() + 1;
            let gap_ratio = (total_span - query.len()) as f32 / path_chars.len() as f32;
            let gap_penalty = gap_ratio * 0.10;

            let score =
                (base_score + consecutive_bonus + position_bonus - gap_penalty).clamp(0.50, 0.88);

            return Some(AbbreviationMatch {
                score,
                strategy: MatchStrategy::Sequential,
                matched_indices,
            });
        }

        None
    }

    /// Tokenize a path into meaningful components.
    ///
    /// Splits on: / \ . - _ (space)
    /// Preserves: individual words and separates extension
    fn tokenize_path(path: &str) -> Vec<String> {
        let mut tokens = Vec::new();

        // Split path into components first
        for component in Path::new(path).components() {
            if let Some(comp_str) = component.as_os_str().to_str() {
                // Further split on -, _, .
                let parts: Vec<&str> = comp_str
                    .split(['-', '_', '.'])
                    .filter(|s| !s.is_empty())
                    .collect();

                tokens.extend(parts.iter().map(|s| s.to_string()));
            }
        }

        tokens
    }

    /// Calculate bonus for consecutive matches.
    fn calculate_consecutive_bonus(&self, indices: &[usize], query_len: usize) -> f32 {
        if indices.len() <= 1 {
            return 0.0;
        }

        let mut consecutive_count = 0;
        for window in indices.windows(2) {
            if window[1] == window[0] + 1 {
                consecutive_count += 1;
            }
        }

        (consecutive_count as f32 / query_len as f32) * 0.05
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_prefix_filename() {
        let matcher = AbbreviationMatcher::new();

        let result = matcher.match_path("main", "src/main.rs");
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.strategy, MatchStrategy::ExactPrefix);
        assert!(m.score >= 0.98);
    }

    #[test]
    fn test_exact_prefix_component() {
        let matcher = AbbreviationMatcher::new();

        let result = matcher.match_path("src", "src/main.rs");
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.strategy, MatchStrategy::ExactPrefix);
    }

    #[test]
    fn test_component_first_letter() {
        let matcher = AbbreviationMatcher::new();

        // "vcs" should match "vicaya-core/src/main.rs"
        let result = matcher.match_path("vcs", "vicaya-core/src/main.rs");
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.strategy, MatchStrategy::ComponentFirst);
        assert!(m.score >= 0.90);
    }

    #[test]
    fn test_component_first_letter_vcm() {
        let matcher = AbbreviationMatcher::new();

        // "vcm" should match "vicaya-core/src/main.rs"
        let result = matcher.match_path("vcm", "vicaya-core/src/main.rs");
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.strategy, MatchStrategy::ComponentFirst);
    }

    #[test]
    fn test_camelcase_matching() {
        let matcher = AbbreviationMatcher::new();

        // "ct" should match "Cargo.toml"
        let result = matcher.match_path("ct", "Cargo.toml");
        assert!(result.is_some());
        let m = result.unwrap();
        // Could be CamelCase or ComponentFirst
        assert!(m.score >= 0.85);
    }

    #[test]
    fn test_sequential_matching() {
        let matcher = AbbreviationMatcher::new();

        // "main" should match "admin/main.rs"
        let result = matcher.match_path("main", "admin/main.rs");
        assert!(result.is_some());
        let m = result.unwrap();
        // Could be Sequential or ExactPrefix
        assert!(m.score >= 0.70);
    }

    #[test]
    fn test_no_match() {
        let matcher = AbbreviationMatcher::new();

        let result = matcher.match_path("xyz", "vicaya-core/src/main.rs");
        assert!(result.is_none());
    }

    #[test]
    fn test_empty_query() {
        let matcher = AbbreviationMatcher::new();

        let result = matcher.match_path("", "any/path.rs");
        assert!(result.is_none());
    }

    #[test]
    fn test_case_insensitive_default() {
        let matcher = AbbreviationMatcher::new();

        let result = matcher.match_path("MAIN", "src/main.rs");
        assert!(result.is_some());
    }

    #[test]
    fn test_single_char_query() {
        let matcher = AbbreviationMatcher::new();

        let result = matcher.match_path("m", "src/main.rs");
        assert!(result.is_some());
    }

    #[test]
    fn test_tokenize_path() {
        let tokens = AbbreviationMatcher::tokenize_path("vicaya-core/src/main.rs");
        assert_eq!(tokens, vec!["vicaya", "core", "src", "main", "rs"]);
    }

    #[test]
    fn test_tokenize_underscores() {
        let tokens = AbbreviationMatcher::tokenize_path("test_file_name.txt");
        assert_eq!(tokens, vec!["test", "file", "name", "txt"]);
    }

    // ===== Real-world test cases from plan =====

    #[test]
    fn test_realworld_vcaya_paths() {
        let matcher = AbbreviationMatcher::new();

        // vcs → vicaya-core/src
        let result = matcher.match_path("vcs", "vicaya-core/src/lib.rs");
        assert!(result.is_some());
        assert!(result.unwrap().score >= 0.90);

        // vcm → vicaya-core/main.rs
        let result = matcher.match_path("vcm", "vicaya-core/src/main.rs");
        assert!(result.is_some());
        assert!(result.unwrap().score >= 0.90);

        // vsm → vicaya-scanner/main.rs
        let result = matcher.match_path("vsm", "vicaya-scanner/src/main.rs");
        assert!(result.is_some());
        assert!(result.unwrap().score >= 0.90);
    }

    #[test]
    fn test_realworld_cargo_toml() {
        let matcher = AbbreviationMatcher::new();

        // CT → Cargo.toml (CamelCase)
        let result = matcher.match_path("CT", "Cargo.toml");
        assert!(result.is_some());
        let m = result.unwrap();
        assert!(m.score >= 0.85);

        // cargo → Cargo.toml (exact prefix)
        let result = matcher.match_path("cargo", "Cargo.toml");
        assert!(result.is_some());
        assert!(result.unwrap().score >= 0.95);
    }

    #[test]
    fn test_realworld_config_files() {
        let matcher = AbbreviationMatcher::new();

        // abc → admin/backup/config.toml
        let result = matcher.match_path("abc", "admin/backup/config.toml");
        assert!(result.is_some());
        assert!(result.unwrap().score >= 0.90);
    }

    #[test]
    fn test_no_match_xyz() {
        let matcher = AbbreviationMatcher::new();

        // xyz should not match vicaya paths
        let result = matcher.match_path("xyz", "vicaya-core/src/main.rs");
        assert!(result.is_none());
    }

    #[test]
    fn test_numbers_in_path() {
        let matcher = AbbreviationMatcher::new();

        let result = matcher.match_path("test", "test123.txt");
        assert!(result.is_some());
        assert!(result.unwrap().score >= 0.90);
    }

    #[test]
    fn test_unicode_paths() {
        let matcher = AbbreviationMatcher::new();

        // Should handle Unicode gracefully
        let result = matcher.match_path("test", "日本語/test.txt");
        assert!(result.is_some());
    }

    #[test]
    fn test_special_chars_in_query() {
        let matcher = AbbreviationMatcher::new();

        // Dots in query should match
        let result = matcher.match_path("c.t", "config.toml");
        assert!(result.is_some());
    }

    #[test]
    fn test_very_long_path() {
        let matcher = AbbreviationMatcher::new();

        let long_path = "very/deep/nested/directory/structure/with/many/components/file.txt";
        let result = matcher.match_path("vdn", long_path);
        assert!(result.is_some());
    }

    #[test]
    fn test_score_ordering() {
        let matcher = AbbreviationMatcher::new();

        // Exact prefix should score higher than sequential
        let exact = matcher.match_path("main", "src/main.rs").unwrap();
        let sequential = matcher.match_path("main", "admin/src/file.rs");

        assert!(exact.score > sequential.map_or(0.0, |m| m.score));
    }

    #[test]
    fn test_component_first_better_than_sequential() {
        let matcher = AbbreviationMatcher::new();

        // "abc" as component first letters should score higher than sequential match
        let comp_first = matcher
            .match_path("abc", "alpha/beta/charlie/file.txt")
            .unwrap();
        let sequential = matcher.match_path("abc", "alphabet/file.txt");

        assert!(comp_first.score >= 0.90);
        if let Some(seq) = sequential {
            assert!(comp_first.score > seq.score);
        }
    }

    #[test]
    fn test_extension_matching() {
        let matcher = AbbreviationMatcher::new();

        // Should match extension separately
        let result = matcher.match_path("mr", "main.rs");
        assert!(result.is_some());
        assert!(result.unwrap().score >= 0.90);
    }

    #[test]
    fn test_consecutive_bonus() {
        let matcher = AbbreviationMatcher::new();

        // Consecutive matches should score slightly higher
        let indices = vec![0, 1, 2, 3];
        let bonus = matcher.calculate_consecutive_bonus(&indices, 4);
        assert!(bonus > 0.0);
    }

    #[test]
    fn test_case_sensitive_matcher() {
        let matcher = AbbreviationMatcher::case_sensitive();

        // Should NOT match when case differs
        let result = matcher.match_path("MAIN", "src/main.rs");
        // May still match via sequential but with lower score or no match
        assert!(result.is_none() || result.unwrap().score < 0.90);

        // Should match when case is same
        let result = matcher.match_path("main", "src/main.rs");
        assert!(result.is_some());
    }

    #[test]
    fn test_matched_indices_validity() {
        let matcher = AbbreviationMatcher::new();

        let result = matcher.match_path("vcs", "vicaya-core/src/main.rs");
        assert!(result.is_some());

        let m = result.unwrap();
        // Matched indices should be valid and within bounds
        assert!(!m.matched_indices.is_empty());
        assert!(m.matched_indices.len() <= "vicaya-core/src/main.rs".len());
    }

    #[test]
    fn test_query_longer_than_path() {
        let matcher = AbbreviationMatcher::new();

        // Query longer than path should not match
        let result = matcher.match_path("verylongquerythatdoesntfit", "short.txt");
        assert!(result.is_none());
    }

    #[test]
    fn test_all_separators() {
        let matcher = AbbreviationMatcher::new();

        // Test various path separators
        let result = matcher.match_path("abc", "alpha-beta_charlie.txt");
        assert!(result.is_some());
        assert!(result.unwrap().score >= 0.90);
    }

    #[test]
    fn test_repeated_characters() {
        let matcher = AbbreviationMatcher::new();

        let result = matcher.match_path("aaa", "alpha/alpha/alpha/file.txt");
        assert!(result.is_some());
    }

    #[test]
    fn test_mixed_case_query() {
        let matcher = AbbreviationMatcher::new();

        // Mixed case query should still match (case-insensitive by default)
        let result = matcher.match_path("MaIn", "src/main.rs");
        assert!(result.is_some());
    }
}
