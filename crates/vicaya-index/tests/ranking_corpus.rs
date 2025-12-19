//! Ranking corpus smoke tests.
//!
//! These tests validate that the deterministic corpus + query suite is wired up
//! correctly (i.e., expected hits exist). Ordering/ranking assertions live in
//! dedicated metrics/regression tests.

mod support;

#[test]
fn corpus_smoke_searches_return_expected_hits() {
    let files = support::corpus_files();
    let suite = support::query_suite();
    let (table, arena, trigram_index) = support::build_snapshot(&files);

    for case in suite {
        let results =
            support::run_query(&table, &arena, &trigram_index, case.query, case.scope, 100);
        for expected in case.relevant_paths {
            assert!(
                results.iter().any(|r| r.path == *expected),
                "query {:?} should contain {:?}. got={:?}",
                case.query,
                expected,
                results.iter().map(|r| r.path.as_str()).collect::<Vec<_>>()
            );
        }
    }
}
