//! Ranking regression tests (ordering invariants).

mod support;

#[test]
fn it_ranks_project_files_above_dependency_caches_for_exact_name_ties() {
    let files = support::corpus_files();
    let (table, arena, trigram_index) = support::build_snapshot(&files);

    let results = support::run_query(&table, &arena, &trigram_index, "server.go", 20);
    assert!(
        !results.is_empty(),
        "expected non-empty results for server.go"
    );

    assert_eq!(
        results[0].path,
        "/Users/alice/GolandProjects/spartan-ranker/server.go",
        "expected project server.go to rank first. got={:?}",
        results.iter().map(|r| r.path.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn it_ranks_user_documents_above_caches_for_common_stems() {
    let files = support::corpus_files();
    let (table, arena, trigram_index) = support::build_snapshot(&files);

    let results = support::run_query(&table, &arena, &trigram_index, "invoice", 20);
    assert!(
        !results.is_empty(),
        "expected non-empty results for invoice"
    );

    assert_eq!(
        results[0].path,
        "/Users/alice/Documents/invoice_2024.pdf",
        "expected Documents invoice to rank first. got={:?}",
        results.iter().map(|r| r.path.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn it_ranks_deep_project_files_above_dependency_caches_for_exact_name_ties() {
    let files = support::corpus_files();
    let (table, arena, trigram_index) = support::build_snapshot(&files);

    let results = support::run_query(&table, &arena, &trigram_index, "search.go", 20);
    assert!(
        !results.is_empty(),
        "expected non-empty results for search.go"
    );

    assert_eq!(
        results[0].path,
        "/Users/alice/GolandProjects/spartan-ranker/handlers/search/search.go",
        "expected project search.go to rank first. got={:?}",
        results.iter().map(|r| r.path.as_str()).collect::<Vec<_>>()
    );
}
