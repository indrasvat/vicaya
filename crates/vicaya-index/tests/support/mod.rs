//! Shared ranking evaluation helpers for vicaya-index integration tests.
//!
//! This is intentionally small and deterministic so ranking changes can be
//! measured repeatably, without depending on the developer’s local filesystem.

use vicaya_index::{FileMeta, FileTable, Query, QueryEngine, SearchResult, StringArena};
use vicaya_index::TrigramIndex;

#[derive(Debug, Clone, Copy)]
pub struct TestFile {
    pub path: &'static str,
    pub name: &'static str,
    pub mtime: i64,
    pub size: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct QueryCase {
    pub query: &'static str,
    pub relevant_paths: &'static [&'static str],
}

pub fn build_snapshot(files: &[TestFile]) -> (FileTable, StringArena, TrigramIndex) {
    let mut file_table = FileTable::new();
    let mut arena = StringArena::new();
    let mut trigram_index = TrigramIndex::new();

    for file in files {
        let (path_offset, path_len) = arena.add(file.path);
        let (name_offset, name_len) = arena.add(file.name);

        let meta = FileMeta {
            path_offset,
            path_len,
            name_offset,
            name_len,
            size: file.size,
            mtime: file.mtime,
            dev: 0,
            ino: 0,
        };

        let file_id = file_table.insert(meta);
        trigram_index.add(file_id, file.name);
    }

    (file_table, arena, trigram_index)
}

pub fn corpus_files() -> Vec<TestFile> {
    // Intentionally insert cache/build noise *early* so the baseline (score-only)
    // ranker would tend to surface them first for exact-name ties.
    vec![
        // Go module cache noise.
        TestFile {
            path: "/Users/alice/go/pkg/mod/golang.org/x/net@v0.24.0/websocket/server.go",
            name: "server.go",
            mtime: 1_600_000_000,
            size: 12_345,
        },
        TestFile {
            path: "/Users/alice/go/pkg/mod/cloud.google.com/go@v0.34.0/cmd/go/server.go",
            name: "server.go",
            mtime: 1_600_000_100,
            size: 23_456,
        },
        // Tool state / caches.
        TestFile {
            path: "/Users/alice/Library/Caches/app/cache/invoice_2024.pdf",
            name: "invoice_2024.pdf",
            mtime: 1_650_000_000,
            size: 999_999,
        },
        TestFile {
            path: "/Users/alice/Projects/spartan-ranker/target/debug/build/log.txt",
            name: "log.txt",
            mtime: 1_700_000_100,
            size: 10_000,
        },
        // User-ish content.
        TestFile {
            path: "/Users/alice/Documents/invoice_2024.pdf",
            name: "invoice_2024.pdf",
            mtime: 1_730_000_000,
            size: 123_456,
        },
        TestFile {
            path: "/Users/alice/Documents/taxes_2023.xlsx",
            name: "taxes_2023.xlsx",
            mtime: 1_720_000_000,
            size: 456_789,
        },
        TestFile {
            path: "/Users/alice/Downloads/Screenshot_2025-12-18_19-40-47.png",
            name: "Screenshot_2025-12-18_19-40-47.png",
            mtime: 1_765_000_000,
            size: 2_000_000,
        },
        TestFile {
            path: "/Users/alice/Pictures/IMG_0001.JPG",
            name: "IMG_0001.JPG",
            mtime: 1_740_000_000,
            size: 3_000_000,
        },
        TestFile {
            path: "/Users/alice/Desktop/meeting-notes.txt",
            name: "meeting-notes.txt",
            mtime: 1_750_000_000,
            size: 2_048,
        },
        // Projects (should be favored for exact filenames).
        TestFile {
            path: "/Users/alice/GolandProjects/spartan-ranker/server.go",
            name: "server.go",
            mtime: 1_770_000_000,
            size: 55_555,
        },
        TestFile {
            path: "/Users/alice/GolandProjects/spartan-ranker/README.md",
            name: "README.md",
            mtime: 1_770_000_100,
            size: 4_096,
        },
        TestFile {
            path: "/Users/alice/GolandProjects/spartan-ranker/Dockerfile",
            name: "Dockerfile",
            mtime: 1_770_000_200,
            size: 1_024,
        },
        // Another project to avoid “single-bucket” corpus.
        TestFile {
            path: "/Users/alice/Projects/recipes/notes.txt",
            name: "notes.txt",
            mtime: 1_760_000_000,
            size: 1_000,
        },
    ]
}

pub fn query_suite() -> Vec<QueryCase> {
    vec![
        QueryCase {
            query: "server.go",
            relevant_paths: &["/Users/alice/GolandProjects/spartan-ranker/server.go"],
        },
        QueryCase {
            query: "invoice",
            relevant_paths: &["/Users/alice/Documents/invoice_2024.pdf"],
        },
        QueryCase {
            query: "Screenshot",
            relevant_paths: &["/Users/alice/Downloads/Screenshot_2025-12-18_19-40-47.png"],
        },
        QueryCase {
            query: "IMG_0001",
            relevant_paths: &["/Users/alice/Pictures/IMG_0001.JPG"],
        },
        QueryCase {
            query: "notes",
            relevant_paths: &[
                "/Users/alice/Desktop/meeting-notes.txt",
                "/Users/alice/Projects/recipes/notes.txt",
            ],
        },
    ]
}

pub fn run_query(
    file_table: &FileTable,
    arena: &StringArena,
    trigram_index: &TrigramIndex,
    query: &str,
    limit: usize,
) -> Vec<SearchResult> {
    let engine = QueryEngine::new(file_table, arena, trigram_index);
    engine.search(&Query {
        term: query.to_string(),
        limit,
    })
}
