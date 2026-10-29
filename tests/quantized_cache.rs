mod support;
use std::{fs, path::Path};
use support::TempDir;
use vecnook::{
    Config, EmbeddingSpace, ErrorKind, Metric,
    generation::{
        PublishOptions, Publisher, QuantizedCacheOptions, ScalarKernel, Snapshot,
        remove_quantized_cache,
    },
};

static PROCESS_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn source(path: &Path) -> Snapshot {
    let mut p = Publisher::create(
        path,
        Config::new(3),
        EmbeddingSpace::new("cache", 3, Metric::SquaredL2),
    )
    .unwrap();
    let mut b = p.begin(1, PublishOptions::default()).unwrap();
    b.add(u64::MAX, &[1., -0., f32::from_bits(1)], "원문")
        .unwrap();
    b.commit().unwrap()
}

fn same(
    a: vecnook::generation::GenerationSearchReport,
    b: vecnook::generation::GenerationSearchReport,
) {
    assert_eq!(
        (
            a.generation,
            a.sequence,
            a.complete,
            a.approximate,
            a.eligible_count,
            a.original_distance_computations
        ),
        (
            b.generation,
            b.sequence,
            b.complete,
            b.approximate,
            b.eligible_count,
            b.original_distance_computations
        )
    );
    assert_eq!(a.neighbors.len(), b.neighbors.len());
    for (a, b) in a.neighbors.iter().zip(&b.neighbors) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.distance.to_bits(), b.distance.to_bits());
        assert_eq!(a.metadata, b.metadata);
    }
}
#[test]
fn cached_block_search_is_bit_equal_for_metrics_filters_tails_and_full_candidates() {
    let _gate = PROCESS_GATE.lock().unwrap();
    let temp = TempDir::new();
    for (m, metric) in [Metric::SquaredL2, Metric::Cosine, Metric::InnerProduct]
        .into_iter()
        .enumerate()
    {
        for dimensions in [1, 2, 3, 17, 768, 1536, 4096] {
            let path = temp.path().join(format!("store-{m}-{dimensions}"));
            let cache = temp.path().join(format!("cache-{m}-{dimensions}"));
            let mut publisher = Publisher::create(
                &path,
                Config::new(dimensions).with_metric(metric),
                EmbeddingSpace::new("cache-compare", dimensions, metric),
            )
            .unwrap();
            let mut builder = publisher
                .begin(4, PublishOptions::default().with_segment_bytes(65536))
                .unwrap();
            for n in 0..35 {
                let mut vector: Vec<_> = (0..dimensions)
                    .map(|d| (((n * 13 + d * 7) % 31) as f32 - 15.) / 17.)
                    .collect();
                vector[0] = 0.5 + n as f32 / 100.;
                if dimensions > 1 {
                    vector[1] = -0.;
                }
                let id = if n == 10 {
                    u64::MAX
                } else {
                    (n as u64 + 1) * 10
                };
                builder.add(id, &vector, &format!("원문-{n}")).unwrap();
            }
            let source = builder.commit().unwrap();
            source
                .prepare_quantized_cache(&cache, QuantizedCacheOptions::default())
                .unwrap();
            let row = dimensions + 16;
            for buffer in [row, row * 3 + 2, 4 * 1024 * 1024] {
                let resident = source.quantize().unwrap();
                let cached = source
                    .quantize_cached(
                        &cache,
                        QuantizedCacheOptions::default().with_buffer_bytes(buffer),
                    )
                    .unwrap();
                assert!(cached.buffer_bytes() <= buffer);
                let mut query = vec![0.2; dimensions];
                query[0] = 1.;
                for k in [0, 1, 10] {
                    for limit in [10, 16, 35] {
                        same(
                            resident.search(&query, k, limit).unwrap(),
                            cached.search(&query, k, limit).unwrap(),
                        );
                    }
                }
                for ids in [&[][..], &[u64::MAX, 10, 10, 9999][..]] {
                    same(
                        resident
                            .search_ids(&query, 10, 16, &ScalarKernel, ids)
                            .unwrap(),
                        cached
                            .search_ids(&query, 10, 16, &ScalarKernel, ids)
                            .unwrap(),
                    );
                }
                same(
                    source.search_exact(&query, 10).unwrap(),
                    cached.search(&query, 10, 35).unwrap(),
                );
                assert!(cached.search(&query, 10, 9).is_err());
                assert!(cached.search(&query, 1, 65_537).is_err());
                assert!(cached.search(&[f32::NAN], 1, 16).is_err());
            }
        }
    }
}

#[test]
fn empty_store_and_pending_quota_are_bounded() {
    let _gate = PROCESS_GATE.lock().unwrap();
    let temp = TempDir::new();
    let path = temp.path().join("empty");
    let cache = temp.path().join("cache");
    let publisher = Publisher::create(
        &path,
        Config::new(2),
        EmbeddingSpace::new("empty", 2, Metric::SquaredL2),
    )
    .unwrap();
    drop(publisher);
    let source = Snapshot::open(&path).unwrap();
    source
        .prepare_quantized_cache(&cache, QuantizedCacheOptions::default())
        .unwrap();
    let cached = source
        .quantize_cached(&cache, QuantizedCacheOptions::default())
        .unwrap();
    assert_eq!(cached.buffer_bytes(), 0);
    assert!(cached.search(&[1., 0.], 10, 16).unwrap().complete);
    drop(cached);
    remove_quantized_cache(&cache).unwrap();
    let pending = cache.join("DATA.pending-1-1");
    fs::write(&pending, vec![0; 1024]).unwrap();
    assert!(
        source
            .prepare_quantized_cache(
                &cache,
                QuantizedCacheOptions::default().with_max_cache_bytes(1024)
            )
            .is_err()
    );
    assert!(pending.exists() && !cache.join("DATA").exists());
    assert_eq!(remove_quantized_cache(&cache).unwrap(), 1);
}

#[test]
#[ignore = "subprocess helper paused after a partial cache file is flushed"]
fn cache_prepare_worker() {
    use std::{
        io::Write,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
    };
    use vecnook::generation::{SegmentFile, SegmentReader};
    let Some(root) = std::env::var_os("VECNOOK_CACHE_CRASH_ROOT") else {
        return;
    };
    struct Gate {
        file: SegmentFile,
        armed: Arc<AtomicBool>,
        reads: Arc<AtomicUsize>,
    }
    impl SegmentReader for Gate {
        fn len(&self) -> u64 {
            self.file.len()
        }
        fn read_exact_at(&self, offset: u64, bytes: &mut [u8]) -> std::io::Result<()> {
            if self.armed.load(Ordering::SeqCst) && self.reads.fetch_add(1, Ordering::SeqCst) == 100
            {
                println!("PENDING_FLUSHED");
                std::io::stdout().flush()?;
                let mut line = String::new();
                std::io::stdin().read_line(&mut line)?;
            }
            self.file.read_exact_at(offset, bytes)
        }
    }
    let root = std::path::PathBuf::from(root);
    let armed = Arc::new(AtomicBool::new(false));
    let reads = Arc::new(AtomicUsize::new(0));
    let snapshot = Snapshot::open(root.join("store"))
        .unwrap()
        .with_segment_readers(|file| {
            Ok(Box::new(Gate {
                file,
                armed: armed.clone(),
                reads: reads.clone(),
            }))
        })
        .unwrap();
    armed.store(true, Ordering::SeqCst);
    snapshot
        .prepare_quantized_cache(root.join("cache"), QuantizedCacheOptions::default())
        .unwrap();
}

#[test]
fn killed_preparer_never_announces_partial_data_and_releases_cache_lease() {
    use std::{
        io::{BufRead, BufReader},
        process::{Command, Stdio},
        sync::mpsc,
        time::Duration,
    };
    let _gate = PROCESS_GATE.lock().unwrap();
    let temp = TempDir::new();
    let path = temp.path().join("store");
    let cache = temp.path().join("cache");
    let mut publisher = Publisher::create(
        &path,
        Config::new(768),
        EmbeddingSpace::new("kill", 768, Metric::SquaredL2),
    )
    .unwrap();
    let mut builder = publisher.begin(1, PublishOptions::default()).unwrap();
    for n in 0..128 {
        let mut vector = vec![0.25; 768];
        vector[0] = n as f32 + 1.;
        builder.add(n, &vector, "original").unwrap();
    }
    drop(builder.commit().unwrap());
    drop(publisher);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "cache_prepare_worker",
            "--nocapture",
        ])
        .env("VECNOOK_CACHE_CRASH_ROOT", temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (send, receive) = mpsc::sync_channel(8);
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if send.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    struct Kill(std::process::Child);
    impl Drop for Kill {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = Kill(child);
    loop {
        if receive.recv_timeout(Duration::from_secs(20)).unwrap() == "PENDING_FLUSHED" {
            break;
        }
    }
    assert!(!cache.join("DATA").exists());
    let pending: Vec<_> = fs::read_dir(&cache)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("DATA.pending-")
        })
        .collect();
    assert_eq!(pending.len(), 1);
    assert!(fs::metadata(&pending[0]).unwrap().len() >= 65536);
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let source = Snapshot::open(&path).unwrap();
    assert_eq!(source.len(), 128);
    assert!(
        source
            .quantize_cached(&cache, QuantizedCacheOptions::default())
            .is_err()
    );
    source
        .prepare_quantized_cache(&cache, QuantizedCacheOptions::default())
        .unwrap();
    let cached = source
        .quantize_cached(&cache, QuantizedCacheOptions::default())
        .unwrap();
    same(
        source.search_exact(&vec![0.25; 768], 10).unwrap(),
        cached.search(&vec![0.25; 768], 10, 128).unwrap(),
    );
    drop(cached);
    assert_eq!(remove_quantized_cache(&cache).unwrap(), 2);
}
#[test]
fn cache_is_optional_bounded_and_cannot_replace_original_validation() {
    let _gate = PROCESS_GATE.lock().unwrap();
    let temp = TempDir::new();
    let path = temp.path().join("store");
    let snapshot = source(&path);
    let cache = temp.path().join("cache");
    assert!(
        snapshot
            .prepare_quantized_cache(
                &cache,
                QuantizedCacheOptions::default().with_max_cache_bytes(1)
            )
            .is_err()
    );
    assert!(!cache.exists());
    assert!(
        snapshot
            .prepare_quantized_cache(path.join("cache"), QuantizedCacheOptions::default())
            .is_err()
    );
    snapshot
        .prepare_quantized_cache(&cache, QuantizedCacheOptions::default())
        .unwrap();
    let cached = snapshot
        .quantize_cached(&cache, QuantizedCacheOptions::default())
        .unwrap();
    assert_eq!(cached.code_bytes(), 3);
    assert_eq!(cached.parameter_bytes(), 16);
    assert_eq!(
        cached.snapshot().get(u64::MAX).unwrap().unwrap().vector[1].to_bits(),
        (-0.0f32).to_bits()
    );
    assert_eq!(
        remove_quantized_cache(&cache).unwrap_err().kind(),
        ErrorKind::Locked
    );
    assert_eq!(
        snapshot
            .prepare_quantized_cache(&cache, QuantizedCacheOptions::default())
            .unwrap_err()
            .kind(),
        ErrorKind::Locked
    );
    drop(cached);
    fs::write(cache.join("notes.txt"), "user work").unwrap();
    assert!(remove_quantized_cache(&cache).is_err());
    assert!(cache.join("DATA").exists() && cache.join("notes.txt").exists());
    fs::remove_file(cache.join("notes.txt")).unwrap();
    assert_eq!(remove_quantized_cache(&cache).unwrap(), 1);
    assert!(cache.join("LEASE").exists());
    assert_eq!(snapshot.get(u64::MAX).unwrap().unwrap().metadata, "원문");
    snapshot
        .prepare_quantized_cache(&cache, QuantizedCacheOptions::default())
        .unwrap();
    let other = source(&temp.path().join("other-store"));
    assert!(
        other
            .quantize_cached(&cache, QuantizedCacheOptions::default())
            .is_err()
    );
    let data = fs::read(cache.join("DATA")).unwrap();
    for bad in [data[..data.len() - 1].to_vec(), vec![0; data.len()], {
        let mut b = data.clone();
        b.push(0);
        b
    }] {
        fs::write(cache.join("DATA"), bad).unwrap();
        assert!(
            snapshot
                .quantize_cached(&cache, QuantizedCacheOptions::default())
                .is_err()
        );
    }
    fs::write(cache.join("DATA"), data).unwrap();
    let generation = snapshot.generation();
    drop(snapshot);
    let segment = path
        .join(format!("gen-{generation:016x}"))
        .join("segment-0000.bin");
    let mut bytes = fs::read(&segment).unwrap();
    bytes[10] ^= 1;
    fs::write(segment, bytes).unwrap();
    assert!(Snapshot::open(&path).is_err());
}
