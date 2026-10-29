//! Deterministic public-API trace for comparing two separately built core libraries.
//! Compile this same file with rustc --edition=2024 -O --extern vecnook=<rlib>.
//! With two fvecs arguments it also traces every SciFact exact/HNSW result.
use vecnook::{
    Config, Metric, SearchOptions, SearchReport, SearchStrategy, VectorIndex, bench::read_fvecs,
};

fn emit(label: &str, report: SearchReport) {
    print!(
        "{label}|{:?}|{:?}|{}|{}|{}|{}|",
        report.mode,
        report.reason,
        report.complete,
        report.distance_computations,
        report.eligible_count,
        report.filter_evaluations
    );
    for n in report.neighbors {
        print!("{}:{:016x}:{};", n.id, n.distance.to_bits(), n.metadata);
    }
    println!();
}

fn vector(seed: usize, dimensions: usize) -> Vec<f32> {
    let mut v: Vec<_> = (0..dimensions)
        .map(|d| (((seed * 37 + d * 13 + d * seed) % 257) as f32 - 128.0) / 64.0)
        .collect();
    v[0] = 1.0;
    if dimensions > 1 && seed.is_multiple_of(7) {
        v[dimensions - 1] = -0.0;
    }
    v
}

fn synthetic() -> vecnook::Result<()> {
    for dimensions in [1, 3, 8, 31, 32, 63, 65, 128, 768] {
        for metric in [Metric::SquaredL2, Metric::Cosine, Metric::InnerProduct] {
            let mut config = Config::new(dimensions).with_metric(metric);
            config.m = 5;
            config.ef_construction = 40;
            let mut index = VectorIndex::new(config)?;
            for i in 0_usize..73 {
                let id = if i == 0 { u64::MAX } else { i as u64 * 17 };
                let seed = if i.is_multiple_of(9) { 9 } else { i };
                index.put(
                    id,
                    &vector(seed, dimensions),
                    if i % 3 == 0 { "A" } else { "B" },
                )?;
            }
            for phase in 0..2 {
                if phase == 1 {
                    for i in (1..73).step_by(4) {
                        index.delete(i as u64 * 17);
                    }
                    for i in (2..73).step_by(7) {
                        index.put(i as u64 * 17, &vector(i + 100, dimensions), "A")?;
                    }
                    index.put(u64::MAX, &vector(999, dimensions), "A")?;
                }
                index.check_invariants()?;
                println!("stats|{dimensions}|{metric:?}|{phase}|{:?}", index.stats());
                for q in 0..6 {
                    let query = vector(q + 1000, dimensions);
                    for k in [0, 1, 10, 80] {
                        for strategy in [
                            SearchStrategy::Exact,
                            SearchStrategy::Hnsw,
                            SearchStrategy::Auto,
                        ] {
                            let options = SearchOptions::default()
                                .with_strategy(strategy)
                                .with_ef_search(128)
                                .with_exact_threshold(0)
                                .with_filtered_exact_threshold(0);
                            let prefix =
                                format!("{dimensions}|{metric:?}|{phase}|{q}|{k}|{strategy:?}");
                            emit(&format!("{prefix}|all"), index.search(&query, k, options)?);
                            emit(
                                &format!("{prefix}|metadata"),
                                index.search_metadata(&query, k, options, "A")?,
                            );
                            emit(
                                &format!("{prefix}|predicate"),
                                index.search_filtered(&query, k, options, |r| r.id % 2 == 0)?,
                            );
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn main() -> vecnook::Result<()> {
    synthetic()?;
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() == 2 {
        let base = read_fvecs(&args[0], 100000)?;
        let queries = read_fvecs(&args[1], 10000)?;
        let mut index = VectorIndex::new(Config::new(base[0].len()).with_metric(Metric::Cosine))?;
        for (id, v) in base.iter().enumerate() {
            index.put(id as u64, v, "")?;
        }
        println!("scifact-stats|{:?}", index.stats());
        for (i, q) in queries.iter().enumerate() {
            emit(&format!("scifact|{i}|exact"), index.search_exact(q, 10)?);
            emit(&format!("scifact|{i}|hnsw"), index.search_hnsw(q, 10, 128)?);
        }
    } else if !args.is_empty() {
        return Err(vecnook::Error::InvalidInput(
            "search_trace [base.fvecs query.fvecs]".into(),
        ));
    }
    Ok(())
}
