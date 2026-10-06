//! Export cosine Top-10 IDs/distances for verification by an independent oracle.
use vecnook::{Config, Error, Metric, VectorIndex, bench::read_fvecs};

fn main() -> vecnook::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err(Error::InvalidInput(
            "ground_truth <base.fvecs> <query.fvecs>".into(),
        ));
    }
    let corpus = read_fvecs(&args[0], 100_000)?;
    let queries = read_fvecs(&args[1], 10_000)?;
    let mut index = VectorIndex::new(Config::new(corpus[0].len()).with_metric(Metric::Cosine))?;
    for (id, vector) in corpus.iter().enumerate() {
        index.put(id as u64, vector, "")?;
    }
    print!("[");
    for (offset, query) in queries.iter().enumerate() {
        if offset != 0 {
            print!(",");
        }
        let report = index.search_exact(query, 10)?;
        let neighbors: Vec<_> = report
            .neighbors
            .iter()
            .map(|n| format!("{{\"id\":{},\"distance\":{}}}", n.id, n.distance))
            .collect();
        print!("[{}]", neighbors.join(","));
    }
    println!("]");
    Ok(())
}
