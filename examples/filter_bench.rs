//! Paired old/new Auto policy measurements over identical seeded vectors and queries.
use std::time::Instant;
use vecnook::{Config, Metric, Result, SearchMode, SearchOptions, VectorIndex};

fn random(state: &mut u64) -> f32 {
    *state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
    ((*state >> 32) as u32 as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32
}
fn vectors(n: usize, d: usize, seed: u64) -> Vec<Vec<f32>> {
    let mut state = 42;
    let centers = (0..64)
        .map(|_| (0..d).map(|_| random(&mut state)).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    state = seed;
    (0..n)
        .map(|i| {
            centers[i % 64]
                .iter()
                .map(|&v| v + random(&mut state) * 0.05)
                .collect()
        })
        .collect()
}
fn cosine(a: &[f32], b: &[f32]) -> f64 {
    let (mut dot, mut aa, mut bb) = (0.0, 0.0, 0.0);
    for (&x, &y) in a.iter().zip(b) {
        let (x, y) = (f64::from(x), f64::from(y));
        dot += x * y;
        aa += x * x;
        bb += y * y;
    }
    (1.0 - dot / (aa * bb).sqrt()).clamp(0.0, 2.0)
}
fn main() -> Result<()> {
    let corpus = vectors(50_000, 128, 43);
    let queries = vectors(100, 128, 84);
    let mut index = VectorIndex::new(Config::new(128).with_metric(Metric::Cosine))?;
    for (id, vector) in corpus.iter().enumerate() {
        index.put(
            id as u64,
            vector,
            if id % 100 == 0 { "selected" } else { "other" },
        )?;
    }
    let ids = (0..50_000u64)
        .filter(|id| id % 100 == 0)
        .collect::<Vec<_>>();
    let truths = queries
        .iter()
        .map(|query| {
            let mut scores = ids
                .iter()
                .map(|&id| (id, cosine(&corpus[id as usize], query)))
                .collect::<Vec<_>>();
            scores.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
            scores.truncate(10);
            scores
        })
        .collect::<Vec<_>>();
    for (policy, options) in [
        (
            "v1.0-auto",
            SearchOptions::default().with_filtered_exact_threshold(0),
        ),
        ("v1.1-auto", SearchOptions::default()),
    ] {
        for path in ["ids", "metadata", "predicate"] {
            let search = |query: &[f32]| match path {
                "ids" => index.search_ids(query, 10, options.with_ef_search(1024), &ids),
                "metadata" => {
                    index.search_metadata(query, 10, options.with_ef_search(1024), "selected")
                }
                _ => index
                    .search_filtered(query, 10, options.with_ef_search(1024), |r| r.id % 100 == 0),
            };
            for query in queries.iter().take(10) {
                search(query)?;
            }
            let mut samples = Vec::new();
            let (mut correct, mut exact, mut distances, mut evaluations) =
                (0usize, 0usize, 0usize, 0usize);
            for (query, truth) in queries.iter().zip(&truths) {
                let start = Instant::now();
                let result = search(query)?;
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
                assert!(result.complete);
                assert_eq!(result.eligible_count, 500);
                let mut returned = std::collections::BTreeSet::new();
                for neighbor in &result.neighbors {
                    assert!(returned.insert(neighbor.id));
                    assert!(neighbor.id % 100 == 0);
                    assert!(
                        (neighbor.distance - cosine(&corpus[neighbor.id as usize], query)).abs()
                            < 1e-12
                    );
                    correct += usize::from(neighbor.distance <= truth[9].1 + 1e-12);
                }
                exact += usize::from(result.mode == SearchMode::Exact);
                distances += result.distance_computations;
                evaluations += result.filter_evaluations;
            }
            samples.sort_by(f64::total_cmp);
            println!(
                "{{\"policy\":\"{policy}\",\"path\":\"{path}\",\"count\":50000,\"dimensions\":128,\"queries\":100,\"eligible\":500,\"ef\":1024,\"p50_ms\":{},\"p95_ms\":{},\"p99_ms\":{},\"recall_at_10\":{},\"exact_queries\":{exact},\"mean_distances\":{},\"mean_filter_evaluations\":{}}}",
                samples[49],
                samples[94],
                samples[98],
                correct as f64 / 1000.0,
                distances as f64 / 100.0,
                evaluations as f64 / 100.0
            );
        }
    }
    Ok(())
}
