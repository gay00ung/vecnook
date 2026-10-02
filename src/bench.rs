//! Synthetic and fvecs benchmarks comparing HNSW with exact search.
use std::{
    fs::File,
    io::{BufReader, Read},
    path::Path,
    time::Instant,
};

use crate::{
    Config, Error, IndexStats, Metric, Result, VectorIndex,
    config::{MAX_DIMENSIONS, MAX_RECORDS, MAX_SNAPSHOT_BYTES},
    rng::Rng,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dataset {
    Clustered,
    Uniform,
    File,
}

impl Dataset {
    pub fn name(self) -> &'static str {
        match self {
            Self::Clustered => "clustered",
            Self::Uniform => "uniform",
            Self::File => "fvecs",
        }
    }
}

#[derive(Clone, Debug)]
pub struct BenchConfig {
    pub count: usize,
    pub dimensions: usize,
    pub queries: usize,
    pub ef_search: usize,
    pub seed: u64,
    pub dataset: Dataset,
    pub metric: Metric,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            count: 10_000,
            dimensions: 512,
            queries: 200,
            ef_search: 128,
            seed: 42,
            dataset: Dataset::Clustered,
            metric: Metric::SquaredL2,
        }
    }
}

#[derive(Debug)]
pub struct Timing {
    /// Query count / sum of timed search-call durations; no concurrent clients.
    pub sequential_qps: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
}

impl Timing {
    fn from_samples(mut seconds: Vec<f64>) -> Self {
        let qps = seconds.len() as f64 / seconds.iter().sum::<f64>();
        seconds.sort_by(f64::total_cmp);
        let percentile = |p: f64| {
            seconds[((seconds.len() as f64 * p).ceil() as usize).saturating_sub(1)] * 1000.0
        };
        Self {
            sequential_qps: qps,
            p50_ms: percentile(0.50),
            p95_ms: percentile(0.95),
        }
    }
}

#[derive(Debug)]
pub struct BenchReport {
    pub config: BenchConfig,
    pub k: usize,
    pub build_seconds: f64,
    pub recall_at_k: f64,
    pub incomplete_queries: usize,
    pub exact: Timing,
    pub hnsw: Timing,
    pub mean_exact_computations: f64,
    pub mean_hnsw_computations: f64,
    pub index_stats: IndexStats,
}

/// Build from scratch and compare HNSW with this engine's exact search.
/// Data construction/ingestion, disk I/O and recovery are not query timings.
pub fn run(config: BenchConfig) -> Result<BenchReport> {
    if config.dataset == Dataset::File {
        return Err(Error::InvalidInput(
            "use run_fvecs for file datasets".into(),
        ));
    }
    Config::new(config.dimensions).validate()?;
    if config.count > MAX_RECORDS
        || config.count.saturating_mul(13 + config.dimensions * 4) + 56 > MAX_SNAPSHOT_BYTES
    {
        return Err(Error::InvalidInput(
            "benchmark corpus exceeds storage limits".into(),
        ));
    }
    if config.ef_search < 10.min(config.count) || config.ef_search > 4096 || config.ef_search == 0 {
        return Err(Error::InvalidInput("invalid benchmark efSearch".into()));
    }
    if config.count == 0
        || config.count > MAX_RECORDS
        || config.queries == 0
        || config.queries > 10_000
    {
        return Err(Error::InvalidInput(
            "invalid benchmark corpus/query count".into(),
        ));
    }
    let (corpus, queries) = fixture(&config);
    run_vectors(config, corpus, queries)
}

/// Read a bounded prefix of a little-endian fvecs file. No external loader.
pub fn read_fvecs(path: impl AsRef<Path>, limit: usize) -> Result<Vec<Vec<f32>>> {
    if !(1..=MAX_RECORDS).contains(&limit) {
        return Err(Error::InvalidInput("fvecs limit must be 1..=100000".into()));
    }
    let mut reader = BufReader::new(File::open(path)?);
    let mut vectors = Vec::new();
    let mut dimensions = None;
    let mut allocated = 0;
    while vectors.len() < limit {
        let mut header = [0; 4];
        if reader.read(&mut header[..1])? == 0 {
            break;
        }
        reader
            .read_exact(&mut header[1..])
            .map_err(|_| Error::InvalidInput("truncated fvecs dimension".into()))?;
        let d = u32::from_le_bytes(header) as usize;
        if !(1..=MAX_DIMENSIONS).contains(&d) || dimensions.is_some_and(|expected| expected != d) {
            return Err(Error::InvalidInput(
                "invalid or mixed fvecs dimensions".into(),
            ));
        }
        dimensions = Some(d);
        allocated += d * 4;
        if allocated > MAX_SNAPSHOT_BYTES {
            return Err(Error::InvalidInput(
                "fvecs prefix exceeds memory payload limit".into(),
            ));
        }
        let mut bytes = vec![0; d * 4];
        reader
            .read_exact(&mut bytes)
            .map_err(|_| Error::InvalidInput("truncated fvecs coordinates".into()))?;
        let vector: Vec<_> = bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        if vector.iter().any(|v| !v.is_finite()) {
            return Err(Error::InvalidInput("non-finite fvecs coordinate".into()));
        }
        vectors.push(vector);
    }
    if vectors.is_empty() {
        return Err(Error::InvalidInput("empty fvecs dataset".into()));
    }
    Ok(vectors)
}

pub fn run_fvecs(
    base: impl AsRef<Path>,
    query: impl AsRef<Path>,
    count: usize,
    queries: usize,
    ef: usize,
    metric: Metric,
) -> Result<BenchReport> {
    if !(1..=MAX_RECORDS).contains(&count)
        || !(1..=10_000).contains(&queries)
        || !(1..=4096).contains(&ef)
    {
        return Err(Error::InvalidInput(
            "invalid fvecs count, query limit or efSearch".into(),
        ));
    }
    let corpus = read_fvecs(base, count)?;
    let query_vectors = read_fvecs(query, queries)?;
    let config = BenchConfig {
        count: corpus.len(),
        dimensions: corpus[0].len(),
        queries: query_vectors.len(),
        ef_search: ef,
        dataset: Dataset::File,
        metric,
        ..BenchConfig::default()
    };
    if query_vectors[0].len() != config.dimensions {
        return Err(Error::InvalidInput("corpus/query dimensions differ".into()));
    }
    run_vectors(config, corpus, query_vectors)
}

fn run_vectors(
    config: BenchConfig,
    corpus: Vec<Vec<f32>>,
    queries: Vec<Vec<f32>>,
) -> Result<BenchReport> {
    if config.count == 0 || config.count > 100_000 || config.queries == 0 || config.queries > 10_000
    {
        return Err(Error::InvalidInput(
            "benchmark count must be 1..=100000; queries 1..=10000".into(),
        ));
    }
    let index_config = Config {
        seed: config.seed,
        metric: config.metric,
        ..Config::new(config.dimensions)
    };
    index_config.validate()?;
    if config.count * (13 + config.dimensions * 4) + 56 > MAX_SNAPSHOT_BYTES {
        return Err(Error::InvalidInput(
            "benchmark corpus exceeds snapshot limit".into(),
        ));
    }
    let k = 10.min(config.count);
    if config.ef_search < k || config.ef_search > 4096 {
        return Err(Error::InvalidInput(
            "benchmark efSearch must be min(10, count)..=4096".into(),
        ));
    }
    let mut index = VectorIndex::new(index_config)?;
    let build = Instant::now();
    for (id, vector) in corpus.into_iter().enumerate() {
        index.put(id as u64, &vector, "")?;
    }
    let build_seconds = build.elapsed().as_secs_f64();
    index.check_invariants()?;
    for query in queries.iter().take(10) {
        index.search_exact(query, k)?;
        index.search_hnsw(query, k, config.ef_search)?;
    }
    let mut exact_times = Vec::with_capacity(config.queries);
    let mut truths = Vec::with_capacity(config.queries);
    let mut exact_computations = 0;
    for query in &queries {
        let start = Instant::now();
        let found = index.search_exact(query, k)?;
        exact_times.push(start.elapsed().as_secs_f64());
        exact_computations += found.distance_computations;
        truths.push(
            found
                .neighbors
                .into_iter()
                .map(|n| n.id)
                .collect::<Vec<_>>(),
        );
    }
    let mut ann_times = Vec::with_capacity(config.queries);
    let mut hits = 0;
    let mut ann_computations = 0;
    let mut incomplete_queries = 0;
    for (query, truth) in queries.iter().zip(&truths) {
        let start = Instant::now();
        let found = index.search_hnsw(query, k, config.ef_search)?;
        ann_times.push(start.elapsed().as_secs_f64());
        ann_computations += found.distance_computations;
        incomplete_queries += usize::from(!found.complete);
        hits += found
            .neighbors
            .iter()
            .filter(|n| truth.contains(&n.id))
            .count();
    }
    Ok(BenchReport {
        k,
        build_seconds,
        recall_at_k: hits as f64 / (config.queries * k) as f64,
        incomplete_queries,
        exact: Timing::from_samples(exact_times),
        hnsw: Timing::from_samples(ann_times),
        mean_exact_computations: exact_computations as f64 / config.queries as f64,
        mean_hnsw_computations: ann_computations as f64 / config.queries as f64,
        index_stats: index.stats(),
        config,
    })
}

fn fixture(config: &BenchConfig) -> (Vec<Vec<f32>>, Vec<Vec<f32>>) {
    let mut random = Rng::new(config.seed ^ 0xa0761d6478bd642f);
    let clusters = (config.count / 20).clamp(1, 64);
    let centers: Vec<Vec<f32>> = if config.dataset == Dataset::Clustered {
        (0..clusters)
            .map(|_| {
                (0..config.dimensions)
                    .map(|_| (random.uniform() * 2.0 - 1.0) as f32)
                    .collect()
            })
            .collect()
    } else {
        Vec::new()
    };
    let mut draw = |cluster: usize| -> Vec<f32> {
        (0..config.dimensions)
            .map(|d| {
                let value = random.uniform() * 2.0 - 1.0;
                match config.dataset {
                    Dataset::Uniform | Dataset::File => value as f32,
                    Dataset::Clustered => centers[cluster][d] + (value * 0.04) as f32,
                }
            })
            .collect()
    };
    let corpus = (0..config.count).map(|i| draw(i % clusters)).collect();
    // Further independent draws; never reuse a stored vector as a query.
    let queries = (0..config.queries)
        .map(|i| draw((i + 3) % clusters))
        .collect();
    (corpus, queries)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixture_queries_are_independent_and_reproducible() {
        let cfg = BenchConfig {
            count: 100,
            dimensions: 8,
            queries: 10,
            ..Default::default()
        };
        let first = fixture(&cfg);
        assert_eq!(first, fixture(&cfg));
        for query in &first.1 {
            assert!(!first.0.contains(query));
        }
    }

    #[test]
    fn small_independent_cluster_benchmark_meets_quality_gate() {
        let report = run(BenchConfig {
            count: 400,
            dimensions: 16,
            queries: 40,
            ef_search: 128,
            ..Default::default()
        })
        .unwrap();
        assert!(report.recall_at_k >= 0.95, "Recall={}", report.recall_at_k);
        assert_eq!(report.incomplete_queries, 0);
        assert_eq!(report.mean_exact_computations, 400.0);
        assert!(report.hnsw.sequential_qps > 0.0);
    }
}
