//! Standalone Rust index comparison; dependencies belong in a separate benchmark workspace.
#![forbid(unsafe_code)]
use std::{
    fs::File,
    io::{BufReader, Read},
    time::Instant,
};
use usearch::{Index, IndexOptions, MetricKind, ScalarKind, new_index};
use vecnook::{Config, Metric, VectorIndex};
fn read(path: &str) -> Vec<Vec<f32>> {
    let mut reader = BufReader::new(File::open(path).unwrap());
    let mut rows = Vec::new();
    let mut dimension = None;
    loop {
        let mut d = [0u8; 4];
        let first = reader.read(&mut d[..1]).unwrap();
        if first == 0 {
            break;
        }
        reader.read_exact(&mut d[1..]).unwrap();
        let d = u32::from_le_bytes(d) as usize;
        assert!((1..=4096).contains(&d));
        assert!(dimension.is_none_or(|n| n == d));
        dimension = Some(d);
        let mut bytes = vec![0; 4 * d];
        reader.read_exact(&mut bytes).unwrap();
        rows.push(
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b))
                .collect(),
        );
        assert!(rows.len() <= 100000);
    }
    assert!(!rows.is_empty());
    rows
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
enum Engine {
    V(VectorIndex),
    U(Index),
}
impl Engine {
    fn search(&self, q: &[f32], k: usize, ef: usize) -> Vec<u64> {
        match self {
            Self::V(index) => index
                .search_hnsw(q, k, ef)
                .unwrap()
                .neighbors
                .iter()
                .map(|n| n.id)
                .collect(),
            Self::U(index) => {
                index.change_expansion_search(ef);
                index.search(q, k).unwrap().keys
            }
        }
    }
}
fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    assert_eq!(
        args.len(),
        3,
        "engine vecnook|usearch base.fvecs query.fvecs"
    );
    let base = read(&args[1]);
    let queries = read(&args[2]);
    let d = base[0].len();
    assert!(queries.iter().all(|q| q.len() == d));
    let k = 10.min(base.len());
    let thresholds = queries
        .iter()
        .map(|q| {
            let mut pairs = base
                .iter()
                .enumerate()
                .map(|(i, v)| (cosine(q, v), i))
                .collect::<Vec<_>>();
            pairs.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            pairs[k - 1].0
        })
        .collect::<Vec<_>>();
    let start = Instant::now();
    let engine = match args[0].as_str() {
        "vecnook" => {
            let mut i = VectorIndex::new(Config::new(d).with_metric(Metric::Cosine)).unwrap();
            for (id, v) in base.iter().enumerate() {
                i.put(id as u64, v, "").unwrap();
            }
            Engine::V(i)
        }
        "usearch" => {
            let i = new_index(&IndexOptions {
                dimensions: d,
                metric: MetricKind::Cos,
                quantization: ScalarKind::F32,
                connectivity: 16,
                expansion_add: 200,
                expansion_search: 128,
                multi: false,
            })
            .unwrap();
            i.reserve(base.len()).unwrap();
            for (id, v) in base.iter().enumerate() {
                i.add(id as u64, v).unwrap();
            }
            Engine::U(i)
        }
        _ => panic!("unknown engine"),
    };
    let build = start.elapsed().as_secs_f64();
    for ef in [16, 32, 64, 128, 256, 512, 1024, 2048, 4096] {
        for q in queries.iter().take(20) {
            engine.search(q, k, ef);
        }
        let mut times = Vec::new();
        let mut hits = 0;
        let mut underfilled = 0;
        for (q, &threshold) in queries.iter().zip(&thresholds) {
            let start = Instant::now();
            let ids = engine.search(q, k, ef);
            times.push(start.elapsed().as_secs_f64());
            let unique = ids
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(unique.len(), ids.len());
            if ids.len() < k {
                underfilled += 1;
            }
            hits += ids
                .iter()
                .filter(|&&id| cosine(q, &base[id as usize]) <= threshold + 1e-12)
                .count();
        }
        let qps = queries.len() as f64 / times.iter().sum::<f64>();
        times.sort_by(f64::total_cmp);
        let p = |pct: f64| {
            times[((times.len() as f64 * pct).ceil() as usize).saturating_sub(1)] * 1000.0
        };
        let recall = hits as f64 / (queries.len() * k) as f64;
        println!(
            "{{\"engine\":\"{}\",\"count\":{},\"dimensions\":{},\"queries\":{},\"k\":{},\"ef\":{},\"build_seconds\":{},\"recall_at_10\":{},\"underfilled\":{},\"p50_ms\":{},\"p95_ms\":{},\"p99_ms\":{},\"qps\":{}}}",
            args[0],
            base.len(),
            d,
            queries.len(),
            k,
            ef,
            build,
            recall,
            underfilled,
            p(0.5),
            p(0.95),
            p(0.99),
            qps
        );
        if recall >= 0.99 {
            break;
        }
    }
}
