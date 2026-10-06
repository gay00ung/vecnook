//! Compare eligibility preparation paths on identical generated queries.
use std::time::Instant;
use vecnook::{Config, Result, SearchOptions, VectorIndex};

fn main() -> Result<()> {
    let mut index = VectorIndex::new(Config::new(8))?;
    let mut state = 42u64;
    let mut random = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / (1u32 << 24) as f32
    };
    for id in 0..10_000 {
        let vector: Vec<_> = (0..8).map(|_| random()).collect();
        index.put(id, &vector, &format!("project-{}", id % 200))?;
    }
    let queries: Vec<Vec<f32>> = (0..500)
        .map(|_| (0..8).map(|_| random()).collect())
        .collect();
    for path in [
        "unfiltered",
        "predicate-all",
        "indexed-equality",
        "predicate-equality",
    ] {
        for query in queries.iter().take(20) {
            index.search(query, 10, SearchOptions::default())?;
        }
        let mut samples = Vec::new();
        let mut evaluations = 0;
        let mut distances = 0;
        for query in &queries {
            let start = Instant::now();
            let report = match path {
                "unfiltered" => index.search(query, 10, SearchOptions::default())?,
                "predicate-all" => {
                    index.search_filtered(query, 10, SearchOptions::default(), |_| true)?
                }
                "indexed-equality" => {
                    index.search_metadata(query, 10, SearchOptions::default(), "project-7")?
                }
                _ => index.search_filtered(query, 10, SearchOptions::default(), |r| {
                    r.metadata == "project-7"
                })?,
            };
            samples.push(start.elapsed().as_secs_f64());
            evaluations += report.filter_evaluations;
            distances += report.distance_computations;
        }
        let total: f64 = samples.iter().sum();
        samples.sort_by(f64::total_cmp);
        println!(
            "path={path} count=10000 dimensions=8 queries=500 k=10 ef=128 p50_ms={:.6} p95_ms={:.6} sequential_qps={:.2} mean_filter_evaluations={} mean_distances={}",
            samples[249] * 1000.0,
            samples[474] * 1000.0,
            500.0 / total,
            evaluations / 500,
            distances / 500
        );
    }
    Ok(())
}
