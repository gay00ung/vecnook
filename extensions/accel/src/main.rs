use std::{collections::BTreeSet, process::ExitCode};
use vecnook::{
    Config, EmbeddingSpace, Error, Metric, Result,
    generation::{PublishOptions, Publisher, Snapshot},
};
use vecnook_accel::{AutoKernel, mapped_snapshot};
const HELP: &str = "vecnook-accel: optional SIMD and pinned-generation mmap\nUsage:\n  vecnook-accel demo <new-store>\n  vecnook-accel search <store> <comma-vector> [k=10] [--sq8 candidates] [--mmap]\n\nOutput is JSON v1 with decimal-string IDs. mmap requires immutable generation files;\nuse Publisher for all changes and never edit/truncate segment files directly.\n";
fn main() -> ExitCode {
    match run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
fn parse<T: std::str::FromStr>(s: &str) -> Result<T> {
    s.parse()
        .map_err(|_| Error::InvalidInput(format!("invalid value {s:?}")))
}
fn run(args: &[String]) -> Result<()> {
    if args.is_empty() || ["--help", "help", "-h"].contains(&args[0].as_str()) {
        print!("{HELP}");
        return Ok(());
    }
    if args.len() == 1 && args[0] == "--version" {
        println!("vecnook-accel {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args[0] == "demo" && args.len() == 2 {
        let path = std::path::Path::new(&args[1]);
        let mut publisher = Publisher::create(
            path,
            Config::new(3),
            EmbeddingSpace::new("accel-demo-v1", 3, Metric::SquaredL2),
        )?;
        let mut builder = publisher.begin(1, PublishOptions::default())?;
        builder.add(u64::MAX, &[1., -0., 0.], "original UTF-8 원문")?;
        builder.add(9007199254740993, &[0., 1., 0.], "other")?;
        let plain = builder.commit()?;
        // SAFETY: this CLI exclusively uses Publisher's immutable-generation
        // protocol. Users must not externally modify the mapped store files.
        let old = unsafe { mapped_snapshot(Snapshot::open(path)?) }?;
        let mut next = publisher.begin(2, PublishOptions::default())?;
        next.add(u64::MAX, &[0., 0., 1.], "new")?;
        let current = next.commit()?;
        publisher.collect_garbage()?;
        let kernel = AutoKernel::detect();
        let report = old
            .quantize()?
            .search_with_kernel(&[1., 0., 0.], 1, old.len(), &kernel)?;
        assert_eq!(report.neighbors[0].id, u64::MAX);
        assert_eq!(
            old.get(u64::MAX)?.unwrap().vector[1].to_bits(),
            (-0.0f32).to_bits()
        );
        assert_eq!(current.sequence(), 2);
        assert_eq!(plain.sequence(), 1);
        old.backup(path.with_extension("backup"), PublishOptions::default())?;
        drop(plain);
        drop(old);
        publisher.collect_garbage()?;
        println!(
            "{{\"schema_version\":1,\"passed\":true,\"backend\":\"{:?}\",\"checks\":[\"mmap_originals\",\"simd_exact_rerank\",\"pinned_publication\",\"full_u64\",\"signed_zero\",\"independent_backup\"]}}",
            kernel.acceleration()
        );
        return Ok(());
    }
    if args[0] != "search" || args.len() < 3 {
        return Err(Error::InvalidInput(HELP.into()));
    }
    let mut snapshot = Snapshot::open(&args[1])?;
    let query: Vec<f32> = args[2].split(',').map(parse).collect::<Result<_>>()?;
    let mut k = 10;
    let mut candidates = None;
    let mut mmap = false;
    let mut seen = BTreeSet::new();
    let mut i = 3;
    if args.get(i).is_some_and(|s| !s.starts_with("--")) {
        k = parse(&args[i])?;
        i += 1;
    }
    while i < args.len() {
        if !seen.insert(args[i].as_str()) {
            return Err(Error::InvalidInput("duplicate acceleration option".into()));
        }
        match args[i].as_str() {
            "--mmap" => {
                mmap = true;
                i += 1;
            }
            "--sq8" if i + 1 < args.len() => {
                candidates = Some(parse(&args[i + 1])?);
                i += 2;
            }
            _ => {
                return Err(Error::InvalidInput(
                    "unknown or incomplete acceleration option".into(),
                ));
            }
        }
    }
    if mmap {
        // SAFETY: the requested store is governed by the documented immutable
        // Publisher protocol; this program does not edit segment files.
        snapshot = unsafe { mapped_snapshot(snapshot) }?;
    }
    let kernel = AutoKernel::detect();
    let report = if let Some(candidates) = candidates {
        snapshot
            .quantize()?
            .search_with_kernel(&query, k, candidates, &kernel)?
    } else {
        snapshot.search_filtered(&query, k, &kernel, |_, _| true)?
    };
    let matches = report
        .neighbors
        .iter()
        .map(|n| format!("{{\"id\":\"{}\",\"distance\":{}}}", n.id, n.distance))
        .collect::<Vec<_>>()
        .join(",");
    println!(
        "{{\"schema_version\":1,\"generation\":\"{}\",\"sequence\":\"{}\",\"backend\":\"{:?}\",\"mmap\":{mmap},\"approximate\":{},\"complete\":{},\"eligible_count\":{},\"original_distance_computations\":{},\"neighbors\":[{matches}]}}",
        report.generation,
        report.sequence,
        kernel.acceleration(),
        report.approximate,
        report.complete,
        report.eligible_count,
        report.original_distance_computations
    );
    Ok(())
}
