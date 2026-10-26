//! Fixed-input, separate-process engine comparison; prepare is outside timed runs.
use std::{
    cmp::Ordering,
    collections::BinaryHeap,
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::Path,
    time::Instant,
};
use vecnook::{
    Config, Database, EmbeddingSpace, Metric, SearchOptions, SearchStrategy,
    generation::{PublishOptions, Publisher, ScalarKernel, Snapshot},
};
use vecnook_accel::{AutoKernel, mapped_snapshot};
type AnyResult<T> = Result<T, Box<dyn std::error::Error>>;
#[derive(Clone, Copy)]
struct Rank(f64, u64);
impl PartialEq for Rank {
    fn eq(&self, b: &Self) -> bool {
        self.cmp(b) == Ordering::Equal
    }
}
impl Eq for Rank {}
impl PartialOrd for Rank {
    fn partial_cmp(&self, b: &Self) -> Option<Ordering> {
        Some(self.cmp(b))
    }
}
impl Ord for Rank {
    fn cmp(&self, b: &Self) -> Ordering {
        self.0.total_cmp(&b.0).then(self.1.cmp(&b.1))
    }
}
fn best(heap: &mut BinaryHeap<Rank>, item: Rank) {
    if heap.len() < 10 {
        heap.push(item);
    } else if item < *heap.peek().unwrap() {
        heap.pop();
        heap.push(item);
    }
}
fn vector(reader: &mut impl Read) -> AnyResult<Option<Vec<f32>>> {
    let mut header = [0; 4];
    let n = reader.read(&mut header[..1])?;
    if n == 0 {
        return Ok(None);
    }
    reader.read_exact(&mut header[1..])?;
    let dimension = u32::from_le_bytes(header) as usize;
    if !(1..=4096).contains(&dimension) {
        return Err("invalid fvecs dimension".into());
    }
    let mut bytes = vec![0; dimension * 4];
    reader.read_exact(&mut bytes)?;
    let result: Vec<_> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|v| f32::from_le_bytes(*v))
        .collect();
    if result.iter().any(|v| !v.is_finite()) || result.iter().all(|v| *v == 0.) {
        return Err("invalid cosine fvecs input".into());
    }
    Ok(Some(result))
}
fn queries(path: &Path) -> AnyResult<Vec<Vec<f32>>> {
    let mut file = BufReader::new(File::open(path)?);
    let mut result = Vec::new();
    while let Some(vector) = vector(&mut file)? {
        result.push(vector);
        if result.len() == 120 {
            break;
        }
    }
    if result.len() < 21 {
        return Err(
            "need at least21 independent query vectors (20 calibration, remaining validation)"
                .into(),
        );
    }
    Ok(result)
}
fn reference(a: &[f32], b: &[f32]) -> f64 {
    let mut dot = 0.0f64;
    let mut left = 0.0;
    let mut right = 0.0;
    for (&a, &b) in a.iter().zip(b) {
        let (a, b) = (a as f64, b as f64);
        dot += a * b;
        left += a * a;
        right += b * b;
    }
    (1.0 - dot * left.sqrt().recip() * right.sqrt().recip()).clamp(0., 2.)
}
fn prepare(root: &Path, base: &Path, query: &Path) -> AnyResult<()> {
    fs::create_dir(root)?;
    fs::copy(query, root.join("queries.fvecs"))?;
    let qs = queries(query)?;
    let dim = qs[0].len();
    let config = Config::new(dim).with_metric(Metric::Cosine);
    let mut publisher = Publisher::create(
        root.join("store"),
        config.clone(),
        EmbeddingSpace::new("frozen-engine-benchmark-v1", dim, Metric::Cosine),
    )?;
    let mut builder = publisher.begin(1, PublishOptions::default())?;
    let mut source = BufReader::new(File::open(base)?);
    let mut oracles = vec![BinaryHeap::new(); qs.len()];
    let mut count = 0u64;
    while let Some(v) = vector(&mut source)? {
        if v.len() != dim {
            return Err("mixed input dimensions".into());
        }
        builder.add(count, &v, &format!("original-{count}"))?;
        for (q, heap) in qs.iter().zip(&mut oracles) {
            best(heap, Rank(reference(q, &v), count));
        }
        count += 1;
    }
    let snapshot = builder.commit()?;
    let mut file = BufWriter::new(File::create(root.join("oracle.tsv"))?);
    for heap in oracles {
        let ids = heap
            .into_sorted_vec()
            .iter()
            .map(|row| row.1.to_string())
            .collect::<Vec<_>>()
            .join(",");
        writeln!(file, "{ids}")?;
    }
    file.flush()?;
    // Build the legacy baseline separately, without retaining a duplicate corpus.
    // Its documented256MiB cap is preserved, rather than raised for this test.
    let legacy_supported = count <= 100000 && snapshot.original_bytes() <= 240 * 1024 * 1024;
    if legacy_supported {
        let mut db = Database::create(root.join("legacy"), config)?;
        let mut source = BufReader::new(File::open(base)?);
        let mut id = 0;
        while let Some(v) = vector(&mut source)? {
            db.put(id, &v, &format!("original-{id}"))?;
            id += 1;
        }
        db.checkpoint()?;
    }
    println!(
        "{{\"stage\":\"prepare\",\"count\":{count},\"dimensions\":{dim},\"queries\":{},\"segments\":{},\"original_bytes\":{},\"legacy_supported\":{legacy_supported}}}",
        qs.len(),
        snapshot.segment_count(),
        snapshot.original_bytes()
    );
    Ok(())
}
fn synthetic(root: &Path, count: usize, dim: usize) -> AnyResult<()> {
    fs::create_dir(root)?;
    let mut seed = 42u64;
    for (name, n, offset) in [("base.fvecs", count, 0), ("query.fvecs", 120, count)] {
        let mut file = BufWriter::new(File::create(root.join(name))?);
        for id in offset..offset + n {
            file.write_all(&(dim as u32).to_le_bytes())?;
            for d in 0..dim {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let noise = ((seed >> 32) as u32 as f32 / u32::MAX as f32 - 0.5) * 0.05;
                let value = ((((id % 64) * 71 + d * 13) % 127) as f32 - 63.) / 63. + noise;
                file.write_all(&value.to_le_bytes())?;
            }
        }
        file.flush()?;
    }
    Ok(())
}
fn percentile(v: &[f64], p: f64) -> f64 {
    v[((v.len() - 1) as f64 * p).round() as usize]
}
fn run(root: &Path, mode: &str, budget: usize) -> AnyResult<()> {
    let qs = queries(&root.join("queries.fvecs"))?;
    let oracle: Vec<Vec<u64>> = fs::read_to_string(root.join("oracle.tsv"))?
        .lines()
        .map(|line| line.split(',').map(|v| v.parse().unwrap()).collect())
        .collect();
    let start = Instant::now();
    let kernel = AutoKernel::detect();
    let mut snapshot = None;
    let mut legacy = None;
    if mode == "legacy" {
        legacy = Some(Database::open(root.join("legacy"))?);
    } else {
        let reader = Snapshot::open(root.join("store"))?;
        snapshot = Some(if mode.starts_with("mmap") {
            // SAFETY: this frozen fixture is only read; prepare has exited and
            // no publisher or external file writer overlaps a timed run.
            unsafe { mapped_snapshot(reader) }?
        } else {
            reader
        });
    }
    let open_ms = start.elapsed().as_secs_f64() * 1000.;
    let start = Instant::now();
    let quantized = if mode.ends_with("sq8") {
        Some(snapshot.as_ref().unwrap().quantize()?)
    } else {
        None
    };
    let build_ms = start.elapsed().as_secs_f64() * 1000.;
    let query = |vector: &[f32]| -> AnyResult<Vec<u64>> {
        let result = if let Some(db) = &legacy {
            db.search(
                vector,
                10,
                SearchOptions::default()
                    .with_strategy(SearchStrategy::Hnsw)
                    .with_ef_search(budget),
            )?
            .neighbors
        } else if let Some(quantized) = &quantized {
            quantized
                .search_with_kernel(vector, 10, budget, &kernel)?
                .neighbors
        } else {
            let kernel: &dyn vecnook::generation::DistanceKernel = if mode == "mmap-simd" {
                &kernel
            } else {
                &ScalarKernel
            };
            snapshot
                .as_ref()
                .unwrap()
                .search_filtered(vector, 10, kernel, |_, _| true)?
                .neighbors
        };
        Ok(result.into_iter().map(|n| n.id).collect())
    };
    // Calibration is untimed and disjoint from the final validation queries.
    let mut calibration = 0usize;
    for (v, expected) in qs[..20].iter().zip(&oracle[..20]) {
        let ids = query(v)?;
        calibration += ids.iter().filter(|id| expected.contains(id)).count();
    }
    let mut times = Vec::new();
    let mut correct = 0usize;
    let mut checksum = 0u64;
    for (v, expected) in qs[20..].iter().zip(&oracle[20..]) {
        let start = Instant::now();
        let ids = query(v)?;
        times.push(start.elapsed().as_secs_f64() * 1000.);
        correct += ids.iter().filter(|id| expected.contains(id)).count();
        for id in ids {
            checksum = checksum.wrapping_mul(31).wrapping_add(id);
        }
    }
    times.sort_by(f64::total_cmp);
    let count = times.len();
    let records = snapshot
        .as_ref()
        .map_or_else(|| legacy.as_ref().unwrap().len(), Snapshot::len);
    println!(
        "{{\"stage\":\"query\",\"mode\":\"{mode}\",\"budget\":{budget},\"records\":{records},\"dimensions\":{},\"queries\":{count},\"calibration_queries\":20,\"calibration_recall_at_10\":{},\"recall_at_10\":{},\"open_ms\":{open_ms},\"code_build_ms\":{build_ms},\"code_bytes\":{},\"parameter_bytes\":{},\"backend\":\"{:?}\",\"p50_ms\":{},\"p95_ms\":{},\"p99_ms\":{},\"result_checksum\":\"{checksum}\"}}",
        qs[0].len(),
        calibration as f64 / 200.,
        correct as f64 / (count * 10) as f64,
        quantized.as_ref().map_or(0, |q| q.code_bytes()),
        quantized.as_ref().map_or(0, |q| q.parameter_bytes()),
        kernel.acceleration(),
        percentile(&times, 0.5),
        percentile(&times, 0.95),
        percentile(&times, 0.99)
    );
    Ok(())
}
fn main() -> AnyResult<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("synthetic") if args.len()==4=>synthetic(Path::new(&args[1]),args[2].parse()?,args[3].parse()?),
        Some("prepare") if args.len()==4=>prepare(Path::new(&args[1]),Path::new(&args[2]),Path::new(&args[3])),
        Some("run") if args.len()==4=>run(Path::new(&args[1]),&args[2],args[3].parse()?),
        _=>Err("engine_bench synthetic NEW_INPUT COUNT DIM | prepare NEW_ROOT BASE QUERIES | run ROOT file|mmap|mmap-simd|file-sq8|mmap-sq8|legacy BUDGET".into()),
    }
}
