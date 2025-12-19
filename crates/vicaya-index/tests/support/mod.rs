//! Shared ranking evaluation helpers for vicaya-index integration tests.
//!
//! This is intentionally small and deterministic so ranking changes can be
//! measured repeatably, without depending on the developer’s local filesystem.

#![allow(dead_code)]

use vicaya_index::TrigramIndex;
use vicaya_index::{FileMeta, FileTable, Query, QueryEngine, SearchResult, StringArena};

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
    pub scope: Option<&'static str>,
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
        TestFile {
            path: "/Users/alice/go/pkg/mod/golang.org/x/text@v0.31.0/search/search.go",
            name: "search.go",
            mtime: 1_600_000_200,
            size: 34_567,
        },
        // Xcode DerivedData noise (common on macOS).
        TestFile {
            path: "/Users/alice/Library/Developer/Xcode/DerivedData/MyApp-abc123/Build/Intermediates.noindex/MyApp.build/Debug-iphonesimulator/MyApp.build/Info.plist",
            name: "Info.plist",
            mtime: 1_600_000_250,
            size: 4_000,
        },
        // Build tool caches.
        TestFile {
            path: "/Users/alice/.gradle/caches/modules-2/files-2.1/com.example/foo/1.0.0/foo-1.0.0.pom",
            name: "pom.xml",
            mtime: 1_600_000_260,
            size: 3_333,
        },
        TestFile {
            path: "/Users/alice/.m2/repository/com/example/foo/1.0.0/foo-1.0.0.pom",
            name: "pom.xml",
            mtime: 1_600_000_270,
            size: 3_333,
        },
        TestFile {
            path: "/Users/alice/.nuget/packages/newtonsoft.json/13.0.3/build/net45/Newtonsoft.Json.props",
            name: "Newtonsoft.Json.props",
            mtime: 1_600_000_280,
            size: 1_111,
        },
        // Python environment noise.
        TestFile {
            path: "/Users/alice/Projects/pyapp/.venv/lib/python3.12/site-packages/requests/sessions.py",
            name: "sessions.py",
            mtime: 1_600_000_290,
            size: 9_999,
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
            path: "/Users/alice/GolandProjects/spartan-ranker/handlers/search/search.go",
            name: "search.go",
            mtime: 1_770_000_050,
            size: 22_222,
        },
        TestFile {
            path: "/Users/alice/Projects/ios-app/Info.plist",
            name: "Info.plist",
            mtime: 1_770_000_060,
            size: 5_000,
        },
        TestFile {
            path: "/Users/alice/Projects/java-app/pom.xml",
            name: "pom.xml",
            mtime: 1_770_000_070,
            size: 6_666,
        },
        TestFile {
            path: "/Users/alice/Projects/pyapp/sessions.py",
            name: "sessions.py",
            mtime: 1_770_000_080,
            size: 2_222,
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
        // Scope-boost demo: two equally-good matches in different repos. The
        // out-of-scope file is newer, so without a scope-aware boost it wins.
        TestFile {
            path: "/Users/alice/Projects/other-app/settings.json",
            name: "settings.json",
            mtime: 1_780_000_000,
            size: 1_000,
        },
        TestFile {
            path: "/Users/alice/GolandProjects/spartan-ranker/settings.json",
            name: "settings.json",
            mtime: 1_760_000_000,
            size: 1_000,
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
            scope: None,
            relevant_paths: &["/Users/alice/GolandProjects/spartan-ranker/server.go"],
        },
        QueryCase {
            query: "search.go",
            scope: None,
            relevant_paths: &[
                "/Users/alice/GolandProjects/spartan-ranker/handlers/search/search.go",
            ],
        },
        QueryCase {
            query: "Info.plist",
            scope: None,
            relevant_paths: &["/Users/alice/Projects/ios-app/Info.plist"],
        },
        QueryCase {
            query: "pom.xml",
            scope: None,
            relevant_paths: &["/Users/alice/Projects/java-app/pom.xml"],
        },
        QueryCase {
            query: "sessions.py",
            scope: None,
            relevant_paths: &["/Users/alice/Projects/pyapp/sessions.py"],
        },
        QueryCase {
            query: "settings.json",
            scope: Some("/Users/alice/GolandProjects/spartan-ranker"),
            relevant_paths: &["/Users/alice/GolandProjects/spartan-ranker/settings.json"],
        },
        QueryCase {
            query: "invoice",
            scope: None,
            relevant_paths: &["/Users/alice/Documents/invoice_2024.pdf"],
        },
        QueryCase {
            query: "Screenshot",
            scope: None,
            relevant_paths: &["/Users/alice/Downloads/Screenshot_2025-12-18_19-40-47.png"],
        },
        QueryCase {
            query: "IMG_0001",
            scope: None,
            relevant_paths: &["/Users/alice/Pictures/IMG_0001.JPG"],
        },
        QueryCase {
            query: "notes",
            scope: None,
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
    scope: Option<&str>,
    limit: usize,
) -> Vec<SearchResult> {
    let engine = QueryEngine::new(file_table, arena, trigram_index);
    engine.search(&Query {
        term: query.to_string(),
        limit,
        scope: scope.map(std::path::PathBuf::from),
    })
}
