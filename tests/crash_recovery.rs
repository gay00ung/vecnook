//! Real process interruption after observable WAL growth or temporary snapshot creation.
mod support;
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
use support::TempDir;
use vecnook::{Config, Database, Mutation};

fn coordinates(id: u64) -> [f32; 8] {
    [id as f32, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]
}
fn payload() -> String {
    "payload".repeat(2000)
}

#[test]
#[ignore = "subprocess worker invoked by the parent recovery test"]
fn crash_worker() {
    let path = std::env::var_os("VECNOOK_CRASH_PATH").expect("parent supplies path");
    let phase = std::env::var("VECNOOK_CRASH_PHASE").unwrap();
    let mut db = Database::open(path).unwrap();
    let metadata = payload();
    if phase == "checkpoint" {
        db.put(7, &coordinates(777), &metadata).unwrap();
    }
    if phase == "compact" {
        let vectors: Vec<_> = (0..128).map(|id| coordinates(id + 1000)).collect();
        let operations: Vec<_> = vectors
            .iter()
            .enumerate()
            .map(|(id, vector)| Mutation::Put {
                id: id as u64,
                vector,
                metadata: &metadata,
            })
            .collect();
        db.write_batch(&operations).unwrap();
    }
    println!("READY {}", db.sequence());
    std::io::stdout().flush().unwrap();
    std::io::stdin().read_exact(&mut [0]).unwrap();
    match phase.as_str() {
        "wal" => {
            let vectors: Vec<_> = (10_000..10_400).map(coordinates).collect();
            let operations: Vec<_> = vectors
                .iter()
                .enumerate()
                .map(|(offset, vector)| Mutation::Put {
                    id: 10_000 + offset as u64,
                    vector,
                    metadata: &metadata,
                })
                .collect();
            db.write_batch(&operations).unwrap();
        }
        "checkpoint" => db.checkpoint().unwrap(),
        "compact" => {
            db.compact().unwrap();
        }
        _ => panic!("unknown crash phase"),
    }
    println!("DONE");
    std::io::stdout().flush().unwrap();
    // Keep the process alive so the parent can distinguish an actual kill from exit.
    let _ = std::io::stdin().read_exact(&mut [0]);
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn killed_writes_checkpoints_and_compaction_preserve_every_acknowledged_record() {
    let temp = TempDir::new();
    let base = temp.path().join("base");
    let metadata = payload();
    let mut db = Database::create(&base, Config::new(8)).unwrap();
    for start in (0..512).step_by(128) {
        let vectors: Vec<_> = (start..start + 128).map(coordinates).collect();
        let operations: Vec<_> = vectors
            .iter()
            .enumerate()
            .map(|(offset, vector)| Mutation::Put {
                id: start + offset as u64,
                vector,
                metadata: &metadata,
            })
            .collect();
        db.write_batch(&operations).unwrap();
    }
    db.checkpoint().unwrap();
    drop(db);
    for phase in ["wal", "checkpoint", "compact"] {
        let mut verified = false;
        for attempt in 0..5 {
            let path = temp.path().join(format!("{phase}-{attempt}"));
            fs::create_dir(&path).unwrap();
            for file in ["snapshot.bin", "index.bin", "wal.bin"] {
                fs::copy(base.join(file), path.join(file)).unwrap();
            }
            let child = Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", "crash_worker", "--nocapture"])
                .env("VECNOOK_CRASH_PATH", &path)
                .env("VECNOOK_CRASH_PHASE", phase)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let mut child = ChildGuard(child);
            let stdout = child.0.stdout.take().unwrap();
            let (sender, receiver) = mpsc::channel();
            let reader = std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    if sender.send(line.unwrap()).is_err() {
                        break;
                    }
                }
            });
            let deadline = Instant::now() + Duration::from_secs(20);
            let sequence = loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let line = receiver
                    .recv_timeout(remaining)
                    .expect("bounded wait for acknowledged setup");
                if let Some((_, value)) = line.split_once("READY ") {
                    break value.trim().parse::<u64>().unwrap();
                }
            };
            let wal_before = fs::metadata(path.join("wal.bin")).unwrap().len();
            child.0.stdin.as_mut().unwrap().write_all(b"!").unwrap();
            child.0.stdin.as_mut().unwrap().flush().unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            let observed = loop {
                let changed = if phase == "wal" {
                    fs::metadata(path.join("wal.bin")).is_ok_and(|m| m.len() > wal_before)
                } else {
                    path.join("snapshot.tmp").exists()
                };
                if changed {
                    break true;
                }
                if receiver.try_iter().any(|line| line.contains("DONE"))
                    || Instant::now() >= deadline
                {
                    break false;
                }
                std::thread::yield_now();
            };
            child.0.kill().unwrap();
            let status = child.0.wait().unwrap();
            assert!(!status.success());
            reader.join().unwrap();
            if !observed {
                continue;
            }
            let recovered = Database::open(&path).unwrap();
            assert!(recovered.sequence() >= sequence);
            for id in 0..512 {
                let record = recovered.get(id).expect("acknowledged record must survive");
                let expected = if phase == "checkpoint" && id == 7 {
                    777
                } else if phase == "compact" && id < 128 {
                    id + 1000
                } else {
                    id
                };
                assert_eq!(
                    record.vector,
                    coordinates(expected),
                    "phase={phase}, id={id}"
                );
                assert_eq!(record.metadata, metadata);
            }
            if phase == "wal" {
                let pending = (10_000..10_400)
                    .filter(|id| recovered.get(*id).is_some())
                    .count();
                assert!(
                    pending == 0 || pending == 400,
                    "unacknowledged atomic batch must be all-or-none"
                );
                if pending == 400 {
                    for id in 10_000..10_400 {
                        assert_eq!(recovered.get(id).unwrap().vector, coordinates(id));
                    }
                }
            }
            recovered.check_invariants().unwrap();
            verified = true;
            break;
        }
        assert!(
            verified,
            "could not observe storage transition for phase {phase}"
        );
    }
}
