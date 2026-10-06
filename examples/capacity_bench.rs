//! Synthetic operating-range runner. Fixture vectors/text are not semantic-quality evidence.
#![forbid(unsafe_code)]
use std::{
    fs,
    io::{BufWriter, Write},
    path::PathBuf,
    process::Command,
    time::Instant,
};
use vecnook::{
    Collection, Config, Database, Document, DocumentFilter, DocumentMutation, Metric, Mutation,
    SearchOptions, SearchReport, SearchStrategy,
};
fn random(s: &mut u64) -> f32 {
    *s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
    ((*s >> 32) as u32 as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32
}
fn vectors(n: usize, d: usize, seed: u64) -> Vec<Vec<f32>> {
    let mut s = 42;
    let centers = (0..64)
        .map(|_| (0..d).map(|_| random(&mut s)).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let mut s = seed;
    (0..n)
        .map(|i| {
            centers[i % 64]
                .iter()
                .map(|&v| v + random(&mut s) * 0.05)
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
fn tag(id: usize, name: &str) -> bool {
    match name {
        "none" => false,
        "p001" => id.is_multiple_of(1000),
        "p01" => id.is_multiple_of(100),
        "p10" => id.is_multiple_of(10),
        "all" => true,
        _ => panic!("filter"),
    }
}
enum Handle {
    Raw(Database),
    Docs(Collection),
}
impl Handle {
    fn search(&self, q: &[f32], ids: &[u64], filter: &str, options: SearchOptions) -> SearchReport {
        match self {
            Self::Raw(db) => {
                if filter == "all" {
                    db.search(q, 10, options).unwrap()
                } else {
                    db.search_ids(q, 10, options, ids).unwrap()
                }
            }
            Self::Docs(c) => {
                c.search(
                    q,
                    10,
                    options,
                    DocumentFilter::default().with_tags(&[filter]),
                )
                .unwrap()
                .search
            }
        }
    }
    fn checkpoint(&mut self) {
        match self {
            Self::Raw(db) => db.checkpoint().unwrap(),
            Self::Docs(c) => c.checkpoint().unwrap(),
        }
    }
    fn compact(&mut self) {
        match self {
            Self::Raw(db) => {
                db.compact().unwrap();
            }
            Self::Docs(c) => {
                c.compact().unwrap();
            }
        }
    }
}
fn rss() -> Option<u64> {
    #[cfg(unix)]
    {
        Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(|kb| kb * 1024)
    }
    #[cfg(not(unix))]
    {
        None
    }
}
fn phase(name: &str, seconds: f64) {
    println!(
        "{{\"phase\":\"{name}\",\"seconds\":{seconds},\"observed_rss_bytes\":{}}}",
        rss().map_or("null".to_owned(), |n| n.to_string())
    );
    std::io::stdout().flush().unwrap();
}
#[allow(clippy::too_many_arguments)]
fn quality(
    handle: &Handle,
    base: &[Vec<f32>],
    queries: &[Vec<f32>],
    alive: &[bool],
    filter: &str,
    ef: usize,
    strategy: SearchStrategy,
    label: &str,
) -> f64 {
    let ids = (0..base.len())
        .filter(|&i| alive[i] && tag(i, filter))
        .map(|i| i as u64)
        .collect::<Vec<_>>();
    let k = 10.min(ids.len());
    let thresholds = queries
        .iter()
        .map(|q| {
            let mut v = ids
                .iter()
                .map(|&i| cosine(q, &base[i as usize]))
                .collect::<Vec<_>>();
            v.sort_by(f64::total_cmp);
            v.get(k.saturating_sub(1)).copied().unwrap_or(0.0)
        })
        .collect::<Vec<_>>();
    let options = SearchOptions::default()
        .with_ef_search(ef)
        .with_strategy(strategy);
    for q in queries.iter().take(10) {
        handle.search(q, &ids, filter, options);
    }
    let mut samples = Vec::new();
    let mut hits = 0;
    let mut underfilled = 0;
    let mut computations = 0;
    let mut exact = 0;
    for (q, &cut) in queries.iter().zip(&thresholds) {
        let start = Instant::now();
        let report = handle.search(q, &ids, filter, options);
        samples.push(start.elapsed().as_secs_f64());
        let unique = report
            .neighbors
            .iter()
            .map(|n| n.id)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(unique.len(), report.neighbors.len());
        assert!(report.neighbors.len() <= k);
        computations += report.distance_computations;
        exact += usize::from(report.mode.name() == "exact");
        if report.neighbors.len() < k {
            underfilled += 1;
        }
        assert_eq!(report.eligible_count, ids.len());
        for n in report.neighbors {
            assert!(
                alive[n.id as usize] && tag(n.id as usize, filter),
                "filter/deletion leakage"
            );
            hits += usize::from(cosine(q, &base[n.id as usize]) <= cut + 1e-12);
        }
    }
    let qps = queries.len() as f64 / samples.iter().sum::<f64>();
    samples.sort_by(f64::total_cmp);
    let p =
        |x: f64| samples[((samples.len() as f64 * x).ceil() as usize).saturating_sub(1)] * 1000.0;
    let recall = if k == 0 {
        1.0
    } else {
        hits as f64 / (k * queries.len()) as f64
    };
    let reported_recall = if k == 0 {
        "null".to_owned()
    } else {
        recall.to_string()
    };
    println!(
        "{{\"phase\":\"quality\",\"label\":\"{label}\",\"filter\":\"{filter}\",\"eligible\":{},\"ef\":{ef},\"recall_at_10\":{reported_recall},\"underfilled\":{underfilled},\"exact_queries\":{exact},\"p50_ms\":{},\"p95_ms\":{},\"p99_ms\":{},\"qps\":{qps},\"mean_distance_computations\":{}}}",
        ids.len(),
        p(0.5),
        p(0.95),
        p(0.99),
        computations as f64 / queries.len() as f64
    );
    recall
}
fn write_fvecs(path: &std::path::Path, rows: &[Vec<f32>]) {
    let mut out = BufWriter::new(fs::File::create(path).unwrap());
    for v in rows {
        out.write_all(&(v.len() as u32).to_le_bytes()).unwrap();
        for x in v {
            out.write_all(&x.to_le_bytes()).unwrap();
        }
    }
    out.flush().unwrap();
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 4 {
        return Err("capacity_bench raw|docs count dimensions new-root".into());
    }
    let n: usize = args[1].parse()?;
    let d: usize = args[2].parse()?;
    let root = PathBuf::from(&args[3]);
    let docs = args[0] == "docs";
    if !docs && args[0] != "raw" {
        return Err("mode raw or docs".into());
    }
    if n == 0 || n > 50000 {
        return Err("count must be 1..50000".into());
    }
    Config::new(d).validate()?;
    let text = "Authored fixture text for measuring original-document storage costs. ".repeat(8);
    let documents = (0..if docs { n } else { 0 })
        .map(|i| {
            let mut doc = Document::new(i as u64, &text, format!("source-{}.md", i % 100));
            doc.tags = ["all", "p001", "p01", "p10"]
                .iter()
                .filter(|t| tag(i, t))
                .map(|t| t.to_string())
                .collect();
            doc
        })
        .collect::<Vec<_>>();
    let config = Config::new(d).with_metric(Metric::Cosine);
    let predicted = 56
        + n * (13
            + 4 * d
            + if docs {
                documents[0].to_payload()?.len() + 64
            } else {
                0
            });
    if predicted > 256 * 1024 * 1024 {
        return Err("case exceeds encoded snapshot budget".into());
    }
    let base = vectors(n, d, 43);
    let queries = vectors(100, d, 84);
    if let Some(parent) = root.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(&root)?;
    write_fvecs(&root.join("base.fvecs"), &base);
    write_fvecs(&root.join("query.fvecs"), &queries);
    let start = Instant::now();
    let mut handle = if docs {
        Handle::Docs(Collection::create(
            &root,
            "database",
            config,
            "synthetic-capacity-v1",
        )?)
    } else {
        Handle::Raw(Database::create(root.join("database"), config)?)
    };
    for start in (0..n).step_by(256) {
        let end = (start + 256).min(n);
        match &mut handle {
            Handle::Raw(db) => {
                let ops = (start..end)
                    .map(|i| Mutation::Put {
                        id: i as u64,
                        vector: &base[i],
                        metadata: "",
                    })
                    .collect::<Vec<_>>();
                db.write_batch(&ops)?;
            }
            Handle::Docs(c) => {
                let ops = (start..end)
                    .map(|i| DocumentMutation::Put {
                        document: &documents[i],
                        vector: &base[i],
                    })
                    .collect::<Vec<_>>();
                c.write_batch(&ops)?;
            }
        }
    }
    phase("build_with_wal", start.elapsed().as_secs_f64());
    let alive = vec![true; n];
    assert_eq!(
        quality(
            &handle,
            &base,
            &queries,
            &alive,
            "all",
            128,
            SearchStrategy::Exact,
            "exact_reference"
        ),
        1.0
    );
    let mut chosen = 4096;
    for ef in [64, 128, 256, 512, 1024, 2048, 4096] {
        if quality(
            &handle,
            &base,
            &queries,
            &alive,
            "all",
            ef,
            SearchStrategy::Hnsw,
            "initial",
        ) >= 0.99
        {
            chosen = ef;
            break;
        }
    }
    for filter in ["none", "p001", "p01", "p10", "all"] {
        for ef in [chosen, 128, 256, 512, 1024, 2048, 4096] {
            if ef < chosen {
                continue;
            }
            if quality(
                &handle,
                &base,
                &queries,
                &alive,
                filter,
                ef,
                SearchStrategy::Auto,
                "filtered",
            ) >= 0.99
            {
                break;
            }
        }
    }
    phase("after_search", 0.0);
    let start = Instant::now();
    handle.checkpoint();
    phase("checkpoint", start.elapsed().as_secs_f64());
    drop(handle);
    let start = Instant::now();
    let mut handle = if docs {
        let space = vecnook::EmbeddingSpace::new("synthetic-capacity-v1", d, Metric::Cosine);
        Handle::Docs(Collection::open(&root, "database", &space)?)
    } else {
        Handle::Raw(Database::open(root.join("database"))?)
    };
    phase("cached_reopen", start.elapsed().as_secs_f64());
    let mut alive = alive;
    let mut base = base;
    for i in 0..n {
        if i % 10 == 9 {
            match &mut handle {
                Handle::Raw(db) => {
                    db.delete(i as u64)?;
                }
                Handle::Docs(c) => {
                    c.delete(i as u64)?;
                }
            }
            alive[i] = false;
        } else if i % 10 == 8 {
            base[i][0] += 0.02;
            match &mut handle {
                Handle::Raw(db) => {
                    db.put(i as u64, &base[i], "")?;
                }
                Handle::Docs(c) => {
                    c.put(&documents[i], &base[i])?;
                }
            }
        }
    }
    for ef in [chosen, 128, 256, 512, 1024, 2048, 4096] {
        if ef < chosen {
            continue;
        }
        if quality(
            &handle,
            &base,
            &queries,
            &alive,
            "all",
            ef,
            SearchStrategy::Hnsw,
            "after_churn",
        ) >= 0.99
        {
            chosen = ef;
            break;
        }
    }
    for filter in ["none", "p001", "p01", "p10", "all"] {
        for ef in [chosen, 128, 256, 512, 1024, 2048, 4096] {
            if ef < chosen {
                continue;
            }
            if quality(
                &handle,
                &base,
                &queries,
                &alive,
                filter,
                ef,
                SearchStrategy::Auto,
                "filtered_after_churn",
            ) >= 0.99
            {
                break;
            }
        }
    }
    drop(handle);
    let start = Instant::now();
    let mut handle = if docs {
        let space = vecnook::EmbeddingSpace::new("synthetic-capacity-v1", d, Metric::Cosine);
        Handle::Docs(Collection::open(&root, "database", &space)?)
    } else {
        Handle::Raw(Database::open(root.join("database"))?)
    };
    phase("wal_recovery", start.elapsed().as_secs_f64());
    let start = Instant::now();
    handle.compact();
    phase("compact", start.elapsed().as_secs_f64());
    let c = match &handle {
        Handle::Raw(db) => db.capacity(),
        Handle::Docs(db) => db.capacity(),
    };
    println!(
        "{{\"phase\":\"capacity\",\"active\":{},\"physical\":{},\"snapshot_bytes\":{},\"remaining_bytes\":{}}}",
        c.active_records, c.physical_nodes, c.snapshot_bytes, c.remaining_snapshot_bytes
    );
    if let Handle::Docs(collection) = handle {
        let (client, worker) = vecnook::app::start(collection)?;
        for clients in [1, 4, 8] {
            let start = Instant::now();
            let joins = (0..clients)
                .map(|_| {
                    let client = client.clone();
                    let q = queries[0].clone();
                    std::thread::spawn(move || {
                        let mut samples = Vec::new();
                        for _ in 0..32 {
                            let start = Instant::now();
                            let result = client
                                .submit("synthetic-capacity-v1", q.clone(), 10, None, vec![])?
                                .recv()
                                .unwrap()
                                .unwrap();
                            assert!(result.neighbors.iter().all(|n| n.document.id % 10 != 9));
                            samples.push(start.elapsed().as_secs_f64());
                        }
                        Ok::<_, vecnook::app::SubmitError>(samples)
                    })
                })
                .collect::<Vec<_>>();
            let mut samples = Vec::new();
            for j in joins {
                samples.extend(j.join().unwrap()?);
            }
            let qps = samples.len() as f64 / start.elapsed().as_secs_f64();
            samples.sort_by(f64::total_cmp);
            let p = |x: f64| {
                samples[((samples.len() as f64 * x).ceil() as usize).saturating_sub(1)] * 1000.0
            };
            println!(
                "{{\"phase\":\"clients\",\"clients\":{clients},\"requests\":{},\"qps\":{qps},\"p50_ms\":{},\"p95_ms\":{},\"p99_ms\":{}}}",
                samples.len(),
                p(0.5),
                p(0.95),
                p(0.99)
            );
        }
        drop(client);
        drop(worker.join().unwrap());
    }
    let disk = fs::read_dir(root.join("database"))?
        .map(|e| e.unwrap().metadata().unwrap().len())
        .sum::<u64>();
    println!("{{\"phase\":\"disk\",\"bytes\":{disk}}}");
    Ok(())
}
