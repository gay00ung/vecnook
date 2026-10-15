//! Common public-API workload compiled unchanged against baseline and candidate.
#![forbid(unsafe_code)]
use std::{fs, path::PathBuf, time::Instant};
use vecnook::{Config, Database, Mutation, SearchOptions, SearchStrategy};

fn vector(id: u64, version: u64) -> Vec<f32> {
    let mut seed = id.wrapping_mul(7919).wrapping_add(version * 104729 + 42);
    (0..32).map(|_| { seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1); (seed >> 32) as u32 as f32 / u32::MAX as f32 }).collect()
}
fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mode = &args[0]; let root = PathBuf::from(&args[1]); let n = args[2].parse::<u64>().unwrap();
    let payload = "m".repeat(4096);
    if mode == "prepare" {
        let mut db = Database::create(&root, Config::new(32)).unwrap();
        for (count, version) in [(n, 0), (n / 2, 1)] {
            for start in (0..count).step_by(128) {
                let vectors = (start..(start + 128).min(count)).map(|id| (id, vector(id, version))).collect::<Vec<_>>();
                let ops = vectors.iter().map(|(id, vector)| Mutation::Put { id: *id, vector, metadata: &payload }).collect::<Vec<_>>();
                db.write_batch(&ops).unwrap();
            }
        }
        for start in (0..n / 4).step_by(128) {
            db.write_batch(&(start..(start+128).min(n/4)).map(|id| Mutation::Delete { id }).collect::<Vec<_>>()).unwrap();
        }
        db.checkpoint().unwrap();
        println!("{{\"phase\":\"prepare\",\"active\":{},\"physical\":{}}}", db.len(), db.stats().physical_nodes);
        return;
    }
    let start = Instant::now();
    let mut db = Database::open(&root).unwrap();
    let open_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(db.len(), (n - n/4) as usize);
    let mut phase_ms = open_ms;
    let mut p95_ms = 0.0;
    let mut digest = 0u64;
    if mode == "update" || mode == "delete" {
        let start = Instant::now();
        for first in (n/4..3*n/4).step_by(128) {
            let vectors = (first..(first+128).min(3*n/4)).map(|id| (id, vector(id, 2))).collect::<Vec<_>>();
            let operations = vectors.iter().map(|(id, vector)| if mode == "update" {
                Mutation::Put { id: *id, vector, metadata: &payload }
            } else { Mutation::Delete { id: *id } }).collect::<Vec<_>>();
            db.write_batch(&operations).unwrap();
        }
        phase_ms = start.elapsed().as_secs_f64() * 1000.0;
    } else if mode == "checkpoint" || mode == "compact" {
        let start = Instant::now();
        if mode == "checkpoint" { db.checkpoint().unwrap(); } else { assert_eq!(db.compact().unwrap(), (n/2+n/4) as usize); }
        phase_ms = start.elapsed().as_secs_f64() * 1000.0;
    } else if mode == "search" {
        let mut times = Vec::new();
        let options = SearchOptions::default().with_strategy(SearchStrategy::Hnsw).with_ef_search(128);
        for i in 0..3000 {
            let query = vector(n + i, 0);
            let start = Instant::now();
            let report = db.search(&query, 10, options).unwrap();
            times.push(start.elapsed().as_secs_f64() * 1000.0);
            assert!(report.complete);
            for neighbor in report.neighbors { digest = digest.wrapping_mul(31).wrapping_add(neighbor.id); }
        }
        phase_ms = times.iter().sum();
        times.sort_by(f64::total_cmp); p95_ms = times[2849];
    } else { assert_eq!(mode, "open"); }
    for id in n/4..n {
        if mode == "delete" && id < 3*n/4 { assert!(db.get(id).is_none()); continue; }
        let record = db.get(id).unwrap();
        assert_eq!(record.metadata, payload);
        assert_eq!(record.vector, vector(id, if mode == "update" && id < 3*n/4 { 2 } else { u64::from(id < n/2) }));
    }
    db.check_invariants().unwrap();
    println!("{{\"phase\":\"{mode}\",\"count\":{n},\"dimensions\":32,\"payload_bytes\":4096,\"active\":{},\"physical\":{},\"open_ms\":{open_ms},\"phase_ms\":{phase_ms},\"p95_ms\":{p95_ms},\"ranking_digest\":\"{digest}\",\"snapshot_bytes\":{}}}", db.len(), db.stats().physical_nodes, fs::metadata(root.join("snapshot.bin")).unwrap().len());
}
