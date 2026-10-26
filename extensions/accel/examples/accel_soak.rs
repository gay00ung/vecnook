//! Separate mapped reader for the core mutation soak's generation store.
use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
use vecnook::generation::{DistanceKernel, ScalarKernel, Snapshot};
use vecnook_accel::{AutoKernel, mapped_snapshot};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 4 {
        return Err("accel_soak STORE NEW_PROGRESS_DIR MIN_SECONDS MIN_CHECKED_RECORDS".into());
    }
    let store = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    let minimum_seconds: f64 = args[2].to_str().ok_or("invalid seconds")?.parse()?;
    let minimum_records: u64 = args[3].to_str().ok_or("invalid record count")?.parse()?;
    if !minimum_seconds.is_finite() || minimum_seconds < 0.0 {
        return Err("invalid seconds".into());
    }
    fs::create_dir(&output)?;
    let wait = Instant::now();
    while !store.join("CURRENT").is_file() {
        if wait.elapsed() > Duration::from_secs(300) {
            return Err("core publisher did not create CURRENT".into());
        }
        thread::sleep(Duration::from_millis(100));
    }
    let start = Instant::now();
    let kernel = AutoKernel::detect();
    let mut checked_records = 0u64;
    let mut checks = 0u64;
    let mut generations = 0u64;
    let mut last_generation = None;
    let mut last_progress = Instant::now();
    loop {
        let plain = Snapshot::open(&store)?;
        if !plain.is_empty() {
            let records = plain
                .ids()
                .map(|id| plain.get(id).map(|r| r.unwrap()))
                .collect::<Result<Vec<_>, _>>()?;
            let query = records[0].vector.clone();
            let exact = plain.search_exact(&query, 10)?;
            let generation = plain.generation();
            // SAFETY: only the concurrently running core Publisher writes this
            // store; it creates new immutable files and honors reader pins.
            let mapped = unsafe { mapped_snapshot(plain) }?;
            for expected in &records {
                let actual = mapped.get(expected.id)?.ok_or("missing pinned record")?;
                assert_eq!(actual.metadata, expected.metadata);
                assert!(
                    actual
                        .vector
                        .iter()
                        .zip(&expected.vector)
                        .all(|(a, b)| a.to_bits() == b.to_bits())
                );
                assert_eq!(
                    kernel.dot(&actual.vector, &query).to_bits(),
                    ScalarKernel.dot(&expected.vector, &query).to_bits()
                );
            }
            assert_eq!(
                exact.neighbors,
                mapped
                    .search_filtered(&query, 10, &kernel, |_, _| true)?
                    .neighbors
            );
            assert_eq!(
                exact.neighbors,
                mapped
                    .quantize()?
                    .search_with_kernel(&query, 10, mapped.len().max(10), &kernel)?
                    .neighbors
            );
            checked_records += records.len() as u64;
            checks += 1;
            if last_generation != Some(generation) {
                generations += 1;
                last_generation = Some(generation);
            }
        }
        let elapsed = start.elapsed().as_secs_f64();
        let complete = elapsed >= minimum_seconds && checked_records >= minimum_records;
        if complete || last_progress.elapsed() >= Duration::from_secs(5) {
            let json = format!(
                "{{\"schema_version\":1,\"completed\":{complete},\"elapsed_seconds\":{elapsed},\"checked_records\":{checked_records},\"mapped_checks\":{checks},\"generations_observed\":{generations},\"backend\":\"{:?}\",\"duration_gate_24h\":{},\"records_gate_100k\":{}}}\n",
                kernel.acceleration(),
                elapsed >= 86400.0,
                checked_records >= 100000
            );
            let mut file = File::create(output.join("progress.json"))?;
            file.write_all(json.as_bytes())?;
            file.sync_all()?;
            last_progress = Instant::now();
        }
        if complete {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}
