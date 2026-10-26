use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use vecnook::{
    Config, EmbeddingSpace, Metric,
    generation::{DistanceKernel, PublishOptions, Publisher, ScalarKernel, Snapshot},
};
use vecnook_accel::{AutoKernel, mapped_snapshot};

// Command's Unix fork/exec window can temporarily inherit other test threads'
// file descriptions. Keep the actual process test separate from unrelated pin
// drop/GC assertions; the mapped worker still runs concurrently with its writer.
static PROCESS_GATE: std::sync::RwLock<()> = std::sync::RwLock::new(());

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "vecnook-accel-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn detected_simd_and_scalar_fallback_preserve_ordered_f64_bits() {
    let accelerated = AutoKernel::detect();
    let fallback = AutoKernel::scalar();
    for length in [0, 1, 2, 3, 4, 5, 7, 16, 17, 31, 64, 768, 1536, 4096] {
        for seed in 0..24usize {
            let special = [
                0.0,
                -0.0,
                f32::MAX,
                f32::MIN,
                f32::from_bits(1),
                -f32::from_bits(1),
                1.0,
                -1.0,
            ];
            let a: Vec<_> = (0..length)
                .map(|n| {
                    if seed == 0 {
                        special[n % 8]
                    } else {
                        (((n * 31 + seed * 71) % 199) as f32 - 93.0) / 17.0
                    }
                })
                .collect();
            let b: Vec<_> = (0..length)
                .map(|n| {
                    if seed == 0 {
                        special[(n + 3) % 8]
                    } else {
                        (((n * 67 + seed * 29) % 181) as f32 - 89.0) / 13.0
                    }
                })
                .collect();
            for kernel in [&accelerated as &dyn DistanceKernel, &fallback] {
                assert_eq!(
                    kernel.dot(&a, &b).to_bits(),
                    ScalarKernel.dot(&a, &b).to_bits(),
                    "dot length={length} seed={seed}"
                );
                assert_eq!(
                    kernel.squared_l2(&a, &b).to_bits(),
                    ScalarKernel.squared_l2(&a, &b).to_bits(),
                    "L2 length={length} seed={seed}"
                );
            }
        }
    }
    assert_eq!(
        accelerated.dot(&[-0.0], &[1.0]).to_bits(),
        ScalarKernel.dot(&[-0.0], &[1.0]).to_bits()
    );
    assert_eq!(
        accelerated.dot(&[1., 2., 3.], &[4.]).to_bits(),
        ScalarKernel.dot(&[1., 2., 3.], &[4.]).to_bits()
    );
}

fn write(writer: &mut Publisher, sequence: u64, label: &str) -> Snapshot {
    let mut builder = writer
        .begin(
            sequence,
            PublishOptions::default().with_segment_bytes(65536),
        )
        .unwrap();
    for id in 0..300 {
        let vector: Vec<_> = (0..129)
            .map(|n| ((id * 31 + n * 17) % 199) as f32 - 97.0)
            .collect();
        builder.add(id as u64, &vector, label).unwrap();
    }
    builder.commit().unwrap()
}

#[test]
fn mapped_originals_sq8_simd_and_thread_reads_match_positional_io() {
    let _gate = PROCESS_GATE.read().unwrap();
    let temp = Temp::new();
    for (n, metric) in [Metric::SquaredL2, Metric::Cosine, Metric::InnerProduct]
        .into_iter()
        .enumerate()
    {
        let path = temp.0.join(n.to_string());
        let mut writer = Publisher::create(
            &path,
            Config::new(129).with_metric(metric),
            EmbeddingSpace::new("test", 129, metric),
        )
        .unwrap();
        let plain = write(&mut writer, 1, "원문");
        // SAFETY: this test only uses Publisher, which never modifies pinned files.
        let mapped = Arc::new(unsafe { mapped_snapshot(Snapshot::open(&path).unwrap()) }.unwrap());
        assert_eq!(
            mapped.get(299).unwrap().unwrap(),
            plain.get(299).unwrap().unwrap()
        );
        let query = plain.get(17).unwrap().unwrap().vector;
        let reference = plain.search_exact(&query, 20).unwrap();
        let kernel = AutoKernel::detect();
        assert_eq!(
            reference.neighbors,
            mapped
                .search_filtered(&query, 20, &kernel, |_, _| true)
                .unwrap()
                .neighbors
        );
        assert_eq!(
            reference.neighbors,
            mapped
                .quantize()
                .unwrap()
                .search_with_kernel(&query, 20, mapped.len(), &kernel)
                .unwrap()
                .neighbors
        );
        let next = write(&mut writer, 2, "next");
        writer.collect_garbage().unwrap();
        assert_eq!(mapped.get(299).unwrap().unwrap().metadata, "원문");
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let mapped = mapped.clone();
                let query = &query;
                let reference = &reference;
                scope.spawn(move || {
                    for _ in 0..10 {
                        assert_eq!(
                            reference.neighbors,
                            mapped.search_exact(query, 20).unwrap().neighbors
                        );
                    }
                });
            }
        });
        drop(plain);
        drop(mapped);
        assert_eq!(writer.collect_garbage().unwrap(), 1);
        assert_eq!(next.sequence(), 2);
    }
}

#[test]
#[ignore = "mapped subprocess reader"]
fn mapped_reader_worker() {
    // SAFETY: parent exclusively uses immutable Publisher generations.
    let snapshot = unsafe {
        mapped_snapshot(Snapshot::open(std::env::var_os("VECNOOK_MAP_PATH").unwrap()).unwrap())
    }
    .unwrap();
    println!("MAPPED");
    std::io::stdout().flush().unwrap();
    std::io::stdin().read_exact(&mut [0]).unwrap();
    assert_eq!(snapshot.get(7).unwrap().unwrap().metadata, "old");
    println!("OLD");
    std::io::stdout().flush().unwrap();
    std::io::stdin().read_exact(&mut [0]).unwrap();
}
#[test]
fn process_mmap_pin_survives_new_publication_and_is_reclaimed_after_kill() {
    let _gate = PROCESS_GATE.write().unwrap();
    let temp = Temp::new();
    let path = temp.0.join("store");
    let mut writer = Publisher::create(
        &path,
        Config::new(129),
        EmbeddingSpace::new("test", 129, Metric::SquaredL2),
    )
    .unwrap();
    drop(write(&mut writer, 1, "old"));
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "mapped_reader_worker",
            "--nocapture",
        ])
        .env("VECNOOK_MAP_PATH", &path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if send.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    loop {
        if receive.recv_timeout(Duration::from_secs(20)).unwrap() == "MAPPED" {
            break;
        }
    }
    drop(write(&mut writer, 2, "new"));
    writer.collect_garbage().unwrap();
    child.stdin.as_mut().unwrap().write_all(b"!").unwrap();
    loop {
        if receive.recv_timeout(Duration::from_secs(20)).unwrap() == "OLD" {
            break;
        }
    }
    child.kill().unwrap();
    child.wait().unwrap();
    reader.join().unwrap();
    assert_eq!(writer.collect_garbage().unwrap(), 1);
}
