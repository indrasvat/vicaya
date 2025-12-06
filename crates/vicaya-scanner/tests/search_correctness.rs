//! Integration tests for vicaya search correctness.
//!
//! These tests create actual temporary file structures, build real indexes,
//! and verify that search results are correct.

use std::fs;
use std::path::Path;
use tempfile::TempDir;
use vicaya_core::Config;
use vicaya_index::{Query, QueryEngine};
use vicaya_scanner::Scanner;

/// Helper to create a test config pointing to a temp directory.
fn create_test_config(root: &Path) -> Config {
    Config {
        index_roots: vec![root.to_path_buf()],
        exclusions: vec![".DS_Store".to_string(), ".vicaya-index".to_string()],
        index_path: root.join(".vicaya-index"),
        max_memory_mb: 128,
        performance: vicaya_core::config::PerformanceConfig {
            scanner_threads: 2,
            reconcile_hour: 3,
        },
    }
}

/// Helper to build index and run a search query.
fn search_files(root: &Path, query_str: &str) -> Vec<String> {
    let config = create_test_config(root);
    let scanner = Scanner::new(config);
    let snapshot = scanner.scan().expect("Failed to scan");

    let engine = QueryEngine::new(
        &snapshot.file_table,
        &snapshot.string_arena,
        &snapshot.trigram_index,
    );

    let query = Query {
        term: query_str.to_string(),
        limit: 100,
    };

    let results = engine.search(&query);
    results
        .into_iter()
        .map(|r| {
            r.path
                .strip_prefix(root.to_str().unwrap())
                .unwrap_or(&r.path)
                .to_string()
        })
        .collect()
}

#[test]
fn test_basic_filename_search() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Create test files
    fs::write(root.join("main.rs"), "").unwrap();
    fs::write(root.join("config.toml"), "").unwrap();
    fs::write(root.join("test_main.rs"), "").unwrap();
    fs::write(root.join("readme.md"), "").unwrap();

    // Search for "main"
    let results = search_files(root, "main");

    assert_eq!(results.len(), 2, "Should find 2 files with 'main'");
    assert!(
        results.iter().any(|p| p.contains("main.rs")),
        "Should find main.rs"
    );
    assert!(
        results.iter().any(|p| p.contains("test_main.rs")),
        "Should find test_main.rs"
    );
    assert!(
        !results.iter().any(|p| p.contains("config.toml")),
        "Should not find config.toml"
    );
}

#[test]
fn test_extension_search() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::write(root.join("file1.rs"), "").unwrap();
    fs::write(root.join("file2.rs"), "").unwrap();
    fs::write(root.join("file3.toml"), "").unwrap();
    fs::write(root.join("file4.md"), "").unwrap();

    let results = search_files(root, ".rs");

    assert_eq!(results.len(), 2, "Should find 2 .rs files");
    assert!(results.iter().all(|p| p.ends_with(".rs")));
}

#[test]
fn test_subdirectory_search() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Create nested structure
    fs::create_dir_all(root.join("src/core")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();

    fs::write(root.join("main.rs"), "").unwrap();
    fs::write(root.join("src/lib.rs"), "").unwrap();
    fs::write(root.join("src/core/engine.rs"), "").unwrap();
    fs::write(root.join("tests/integration_test.rs"), "").unwrap();

    let results = search_files(root, ".rs");

    assert_eq!(results.len(), 4, "Should find all 4 .rs files");
    assert!(
        results.iter().any(|p| p.contains("src/core/engine.rs")),
        "Should find files in nested directories"
    );
}

#[test]
fn test_case_insensitive_search() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::write(root.join("main.rs"), "").unwrap();
    fs::write(root.join("MAIN.txt"), "").unwrap();
    fs::write(root.join("Main.cpp"), "").unwrap();
    fs::write(root.join("readme.md"), "").unwrap();

    let results = search_files(root, "main");

    assert_eq!(
        results.len(),
        3,
        "Should find all 3 files (case-insensitive)"
    );
    assert!(results.iter().any(|p| p.contains("main.rs")));
    assert!(results.iter().any(|p| p.contains("MAIN.txt")));
    assert!(results.iter().any(|p| p.contains("Main.cpp")));
}

#[test]
fn test_no_results() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::write(root.join("file1.rs"), "").unwrap();
    fs::write(root.join("file2.toml"), "").unwrap();

    let results = search_files(root, "notfound");

    assert_eq!(results.len(), 0, "Should return empty results");
}

#[test]
fn test_short_query() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::write(root.join("ab.txt"), "").unwrap();
    fs::write(root.join("abc.rs"), "").unwrap();
    fs::write(root.join("xyz.md"), "").unwrap();

    // Short query (< 3 chars) should still work via linear scan
    let results = search_files(root, "ab");

    // Filter to only the files we created (ignore system files that may appear)
    let expected_files: Vec<_> = results
        .iter()
        .filter(|p| p.ends_with("ab.txt") || p.ends_with("abc.rs"))
        .collect();

    assert_eq!(
        expected_files.len(),
        2,
        "Short queries should use linear scan and work. Got: {:?}",
        results
    );
    assert!(results.iter().any(|p| p.ends_with("ab.txt")));
    assert!(results.iter().any(|p| p.ends_with("abc.rs")));
}

#[test]
fn test_special_characters() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::write(root.join("file-name.rs"), "").unwrap();
    fs::write(root.join("file_name.rs"), "").unwrap();
    fs::write(root.join("file.name.rs"), "").unwrap();

    let results = search_files(root, "file-name");
    assert_eq!(results.len(), 1);
    assert!(results[0].contains("file-name.rs"));

    let results = search_files(root, "file_name");
    assert_eq!(results.len(), 1);
    assert!(results[0].contains("file_name.rs"));
}

#[test]
fn test_trigram_matching() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Create files that should match via trigrams
    fs::write(root.join("configuration.toml"), "").unwrap();
    fs::write(root.join("config.rs"), "").unwrap();
    fs::write(root.join("reconfig.sh"), "").unwrap();
    fs::write(root.join("main.rs"), "").unwrap();

    // "config" should match all files containing that substring
    let results = search_files(root, "config");

    assert_eq!(results.len(), 3, "Should match all files with 'config'");
    assert!(!results.iter().any(|p| p.contains("main.rs")));
}

#[test]
fn test_empty_directory() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Empty directory - no files
    let results = search_files(root, "anything");

    assert_eq!(results.len(), 0, "Empty directory should return no results");
}

#[test]
fn test_large_filename() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    let long_name = "a".repeat(200) + ".rs";
    fs::write(root.join(&long_name), "").unwrap();

    let results = search_files(root, "aaaa");

    assert_eq!(results.len(), 1, "Should handle long filenames");
    assert!(results[0].contains(&long_name));
}

#[test]
fn test_exact_match_ranking() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::write(root.join("main.rs"), "").unwrap();
    fs::write(root.join("test_main.rs"), "").unwrap();
    fs::write(root.join("main_test.rs"), "").unwrap();

    let results = search_files(root, "main");

    // "main.rs" should rank higher (prefix match) than "test_main.rs"
    assert_eq!(results.len(), 3);
    // The first result should be the best match (exact stem match)
    assert!(
        results[0].contains("main.rs")
            && !results[0].contains("test_main")
            && !results[0].contains("main_test"),
        "Exact match should rank highest"
    );
}
