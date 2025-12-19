//! Ranking metrics + reporting for the deterministic corpus/query suite.
//!
//! This is meant to be a “dashboard” you can run locally while iterating on
//! ranking. It prints the rank of the first relevant hit per query and an
//! aggregate MRR@10.
//!
//! By default the report is quiet; set `VICAYA_RANKING_REPORT=1` and run:
//!   cargo test -p vicaya-index --test ranking_metrics -- --nocapture

mod support;

fn first_relevant_rank(results: &[vicaya_index::SearchResult], relevant: &[&str]) -> Option<usize> {
    results
        .iter()
        .position(|r| relevant.iter().any(|p| *p == r.path))
        .map(|idx| idx + 1) // 1-based
}

fn mrr_at_k(results_by_query: &[Option<usize>], k: usize) -> f64 {
    let mut sum = 0.0;
    for rank in results_by_query {
        match rank {
            Some(r) if *r <= k => sum += 1.0 / (*r as f64),
            _ => {}
        }
    }
    sum / (results_by_query.len() as f64)
}

fn should_print_report() -> bool {
    std::env::var("VICAYA_RANKING_REPORT")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

#[test]
fn ranking_report_current_baseline() {
    let files = support::corpus_files();
    let suite = support::query_suite();
    let (table, arena, trigram_index) = support::build_snapshot(&files);

    let mut ranks = Vec::with_capacity(suite.len());

    for case in &suite {
        let results = support::run_query(&table, &arena, &trigram_index, case.query, 100);
        let rank = first_relevant_rank(&results, case.relevant_paths);
        ranks.push(rank);

        if should_print_report() {
            println!("\nquery: {:?}", case.query);
            println!("first_relevant_rank: {:?}", rank);
            for (i, r) in results.iter().take(5).enumerate() {
                println!(
                    "  {:>2}. score={:.2} mtime={} path={}",
                    i + 1,
                    r.score,
                    r.mtime,
                    r.path
                );
            }
        }
    }

    let mrr10 = mrr_at_k(&ranks, 10);

    if should_print_report() {
        println!("\nMRR@10: {:.3}", mrr10);
    }

    // Sanity gate: the suite should always have at least one relevant hit in
    // the top-100 for every query. (Ordering improvements are tracked via
    // additional assertions once we implement the contextual tie-breaker.)
    assert!(
        ranks.iter().all(|r| r.is_some()),
        "Every query should have at least one relevant hit. ranks={:?}",
        ranks
    );
}

