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

fn hit_rate_at_k(ranks: &[Option<usize>], k: usize) -> f64 {
    let hits = ranks.iter().filter(|r| r.is_some_and(|v| v <= k)).count();
    hits as f64 / ranks.len() as f64
}

fn is_noise_path(path: &str) -> bool {
    let p = path.to_lowercase();
    // Conservative set; this is for evaluation, not the full production list.
    p.contains("/go/pkg/mod/")
        || p.contains("/library/caches/")
        || p.contains("/node_modules/")
        || p.contains("/target/")
        || p.contains("/dist/")
        || p.contains("/build/")
        || p.contains("/.git/")
        || p.contains("/.idea/")
}

fn bucket_id(path: &str) -> String {
    // Top-level bucket under a home dir (macOS-ish paths).
    // Example: /Users/alice/GolandProjects/... -> "GolandProjects"
    let comps: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
    if comps.len() >= 3 && comps[0] == "Users" {
        return comps[2].to_string();
    }
    comps.first().copied().unwrap_or("unknown").to_string()
}

fn noise_before_first_relevant(results: &[vicaya_index::SearchResult], relevant: &[&str]) -> usize {
    let Some(rank) = first_relevant_rank(results, relevant) else {
        return 0;
    };
    results
        .iter()
        .take(rank.saturating_sub(1))
        .filter(|r| is_noise_path(&r.path))
        .count()
}

fn should_print_report() -> bool {
    std::env::var("VICAYA_RANKING_REPORT")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn baseline_sort_key(
    result: &vicaya_index::SearchResult,
    path_to_file_id: &std::collections::HashMap<&str, u32>,
) -> (f32, u32) {
    // Higher score first; ties broken by insertion order (file_id).
    let file_id = *path_to_file_id
        .get(result.path.as_str())
        .expect("all result paths should exist in corpus");
    (result.score, file_id)
}

fn render_rank_bar(rank: Option<usize>) -> String {
    match rank {
        Some(r) => "#".repeat(r.min(20)),
        None => "∅".to_string(),
    }
}

#[test]
fn ranking_report_current_baseline() {
    let files = support::corpus_files();
    let suite = support::query_suite();
    let (table, arena, trigram_index) = support::build_snapshot(&files);

    let path_to_file_id: std::collections::HashMap<&str, u32> = files
        .iter()
        .enumerate()
        .map(|(idx, f)| (f.path, idx as u32))
        .collect();

    let mut baseline_ranks = Vec::with_capacity(suite.len());
    let mut current_ranks = Vec::with_capacity(suite.len());
    let mut baseline_noise_before = Vec::with_capacity(suite.len());
    let mut current_noise_before = Vec::with_capacity(suite.len());

    if should_print_report() {
        println!(
            "\n{: <12} {: >8} {: >8} {: >6} {: >6}  top1 (base → curr)",
            "query", "base", "curr", "Δ", "noise"
        );
        println!("{}", "─".repeat(80));
    }

    for case in &suite {
        let current_results =
            support::run_query(&table, &arena, &trigram_index, case.query, case.scope, 100);
        let mut baseline_results = current_results.clone();

        // Baseline: score-only, stable by insertion order (file_id).
        baseline_results.sort_by(|a, b| {
            let ka = baseline_sort_key(a, &path_to_file_id);
            let kb = baseline_sort_key(b, &path_to_file_id);
            kb.0.partial_cmp(&ka.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| ka.1.cmp(&kb.1))
        });

        let baseline_rank = first_relevant_rank(&baseline_results, case.relevant_paths);
        let current_rank = first_relevant_rank(&current_results, case.relevant_paths);

        baseline_ranks.push(baseline_rank);
        current_ranks.push(current_rank);

        let baseline_noise = noise_before_first_relevant(&baseline_results, case.relevant_paths);
        let current_noise = noise_before_first_relevant(&current_results, case.relevant_paths);
        baseline_noise_before.push(baseline_noise);
        current_noise_before.push(current_noise);

        if should_print_report() {
            let delta = match (baseline_rank, current_rank) {
                (Some(b), Some(c)) => (b as i64 - c as i64).to_string(),
                _ => "n/a".to_string(),
            };
            let base_top = baseline_results
                .first()
                .map(|r| r.path.as_str())
                .unwrap_or("");
            let curr_top = current_results
                .first()
                .map(|r| r.path.as_str())
                .unwrap_or("");
            println!(
                "{: <12} {: >8?} {: >8?} {: >6} {: >6}  {} → {}",
                case.query,
                baseline_rank,
                current_rank,
                delta,
                baseline_noise.saturating_sub(current_noise),
                base_top,
                curr_top
            );

            println!(
                "  base {: <22} curr {: <22}",
                render_rank_bar(baseline_rank),
                render_rank_bar(current_rank)
            );
        }
    }

    let baseline_mrr10 = mrr_at_k(&baseline_ranks, 10);
    let current_mrr10 = mrr_at_k(&current_ranks, 10);
    let baseline_hit1 = hit_rate_at_k(&baseline_ranks, 1);
    let current_hit1 = hit_rate_at_k(&current_ranks, 1);
    let baseline_hit3 = hit_rate_at_k(&baseline_ranks, 3);
    let current_hit3 = hit_rate_at_k(&current_ranks, 3);

    let baseline_noise_avg: f64 =
        baseline_noise_before.iter().sum::<usize>() as f64 / baseline_noise_before.len() as f64;
    let current_noise_avg: f64 =
        current_noise_before.iter().sum::<usize>() as f64 / current_noise_before.len() as f64;

    if should_print_report() {
        // Diversity: average unique home-buckets in the top-10.
        let mut base_div = Vec::new();
        let mut curr_div = Vec::new();
        for case in &suite {
            let results =
                support::run_query(&table, &arena, &trigram_index, case.query, case.scope, 100);
            let mut baseline_results = results.clone();

            baseline_results.sort_by(|a, b| {
                let ka = baseline_sort_key(a, &path_to_file_id);
                let kb = baseline_sort_key(b, &path_to_file_id);
                kb.0.partial_cmp(&ka.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| ka.1.cmp(&kb.1))
            });
            let base_buckets: std::collections::BTreeSet<String> = baseline_results
                .iter()
                .take(10)
                .map(|r| bucket_id(&r.path))
                .collect();
            base_div.push(base_buckets.len());

            let curr_buckets: std::collections::BTreeSet<String> = results
                .iter()
                .take(10)
                .map(|r| bucket_id(&r.path))
                .collect();
            curr_div.push(curr_buckets.len());
        }
        let base_div_avg: f64 = base_div.iter().sum::<usize>() as f64 / base_div.len() as f64;
        let curr_div_avg: f64 = curr_div.iter().sum::<usize>() as f64 / curr_div.len() as f64;

        println!("\n{: <18} {: >10} {: >10}", "metric", "baseline", "current");
        println!("{}", "─".repeat(44));
        println!(
            "{: <18} {: >10.3} {: >10.3}",
            "MRR@10", baseline_mrr10, current_mrr10
        );
        println!(
            "{: <18} {: >10.3} {: >10.3}",
            "Hit@1", baseline_hit1, current_hit1
        );
        println!(
            "{: <18} {: >10.3} {: >10.3}",
            "Hit@3", baseline_hit3, current_hit3
        );
        println!(
            "{: <18} {: >10.3} {: >10.3}",
            "NoiseBeforeRel", baseline_noise_avg, current_noise_avg
        );
        println!(
            "{: <18} {: >10.3} {: >10.3}",
            "Diversity@10", base_div_avg, curr_div_avg
        );
    }

    // Regression gates (current should not be worse than baseline).
    assert!(
        current_mrr10 >= baseline_mrr10,
        "MRR@10 regressed: baseline={:.3} current={:.3}",
        baseline_mrr10,
        current_mrr10
    );
    assert!(
        current_hit1 >= baseline_hit1,
        "Hit@1 regressed: baseline={:.3} current={:.3}",
        baseline_hit1,
        current_hit1
    );
    assert!(
        current_hit3 >= baseline_hit3,
        "Hit@3 regressed: baseline={:.3} current={:.3}",
        baseline_hit3,
        current_hit3
    );
    assert!(
        current_noise_avg <= baseline_noise_avg,
        "Noise-before-first-relevant regressed: baseline={:.3} current={:.3}",
        baseline_noise_avg,
        current_noise_avg
    );

    // Sanity gate: the suite should always have at least one relevant hit in
    // the top-100 for every query. (Ordering improvements are tracked via
    // additional assertions once we implement the contextual tie-breaker.)
    assert!(
        baseline_ranks.iter().all(|r| r.is_some()) && current_ranks.iter().all(|r| r.is_some()),
        "Every query should have at least one relevant hit. ranks={:?}",
        baseline_ranks
    );
}
