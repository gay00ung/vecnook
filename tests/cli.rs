mod support;
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    time::Duration,
};
use support::TempDir;
use vecnook::{Database, Error, Metric};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vecnook"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn real_cli_initializes_changes_searches_and_reopens() {
    let temp = TempDir::new();
    let path = temp.path().to_str().unwrap();
    assert!(run(&["init", path, "2"]).status.success());
    assert!(run(&["put", path, "1", "0,0", "첫 번째"]).status.success());
    assert!(run(&["put", path, "2", "3,4", "second"]).status.success());
    let search = run(&["search", path, "0,0", "2", "128", "hnsw"]);
    assert!(search.status.success());
    let output = String::from_utf8(search.stdout).unwrap();
    assert!(output.contains("mode=hnsw"));
    assert!(output.contains("returned=2 complete=true"));
    assert!(output.contains("id=1 squared_l2=0.000000000"));
    assert!(output.contains("id=2 squared_l2=25.000000000"));
    assert!(run(&["delete", path, "1"]).status.success());
    assert!(!run(&["get", path, "1"]).status.success());
    assert!(run(&["checkpoint", path]).status.success());
    assert!(run(&["compact", path]).status.success());
    let db = Database::open(temp.path()).unwrap();
    assert_eq!(db.stats().active_records, 1);
    assert_eq!(db.stats().physical_nodes, 1);
}

#[test]
fn bad_cli_arguments_have_failure_exit_codes() {
    assert!(!run(&["does-not-exist", "not-a-db"]).status.success());
    assert!(!run(&["init"]).status.success());
    assert!(!run(&["bench", "0"]).status.success());
    let temp = TempDir::new();
    let path = temp.path().to_str().unwrap();
    assert!(run(&["init", path, "2"]).status.success());
    assert!(!run(&["put", path, "bad-id", "0,0"]).status.success());
    assert!(!run(&["put", path, "1", "NaN,0"]).status.success());
    assert!(!run(&["put", path, "1", "0"]).status.success());
    assert!(
        !run(&["search", path, "0,0", "10", "128", "unknown"])
            .status
            .success()
    );
}

struct ManagedChild(Child);
impl Drop for ManagedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn acknowledged_changes_survive_actual_process_kill_without_checkpoint() {
    let temp = TempDir::new();
    let path = temp.path().to_str().unwrap();
    assert!(run(&["init", path, "2"]).status.success());
    let child = Command::new(env!("CARGO_BIN_EXE_vecnook"))
        .args(["shell", path])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut child = ManagedChild(child);
    let stdout = child.0.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if sender.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    let next = || {
        receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("bounded wait for shell acknowledgement")
    };
    assert_eq!(next(), "READY dimensions=2");
    assert!(matches!(Database::open(temp.path()), Err(Error::Locked)));
    for (command, expected) in [
        ("put 1 1,2 original", "OK inserted"),
        ("put 1 5,6 updated 한글", "OK updated"),
        ("put 2 3,4 temporary", "OK inserted"),
        ("delete 2", "OK deleted"),
    ] {
        writeln!(child.0.stdin.as_mut().unwrap(), "{command}").unwrap();
        child.0.stdin.as_mut().unwrap().flush().unwrap();
        assert!(next().starts_with(expected));
    }
    child.0.kill().unwrap();
    let status = child.0.wait().unwrap();
    assert!(!status.success());
    reader.join().unwrap();
    let db = Database::open(temp.path()).unwrap();
    assert_eq!(db.recovery_info().replayed_frames, 4);
    assert_eq!(db.get(1).unwrap().vector, [5.0, 6.0]);
    assert_eq!(db.get(1).unwrap().metadata, "updated 한글");
    assert!(db.get(2).is_none());
}

#[test]
fn cli_metric_batch_filtered_search_backup_and_version_are_usable() {
    let temp = TempDir::new();
    let source = temp.path().join("source");
    let backup = temp.path().join("backup");
    let batch = temp.path().join("batch.tsv");
    let path = source.to_str().unwrap();
    let file = batch.to_str().unwrap();
    let version = run(&["--version"]);
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        format!("vecnook {}", env!("CARGO_PKG_VERSION"))
    );
    assert!(
        run(&["init", path, "2", "--metric", "cosine"])
            .status
            .success()
    );
    std::fs::write(
        &batch,
        "put\t1\t1,0\ttenant a\nput\t2\t0,1\ttenant b\nput\t1\t2,0\ttenant a\ndelete\t3\n",
    )
    .unwrap();
    let result = run(&["batch", path, file]);
    assert!(result.status.success());
    assert!(
        String::from_utf8(result.stdout)
            .unwrap()
            .contains("sequence=1 inserted=2 updated=1 deleted=0 absent=1")
    );
    let result = run(&[
        "search",
        path,
        "1,0",
        "10",
        "128",
        "auto",
        "--metadata",
        "tenant b",
    ]);
    assert!(result.status.success());
    let output = String::from_utf8(result.stdout).unwrap();
    assert!(output.contains("mode=exact") && output.contains("eligible=1"));
    assert!(
        output.contains("reason=SmallEligibleSet") && output.contains("id=2 cosine=1.000000000")
    );
    assert!(!output.contains("id=1 "));
    let default_search = run(&["search", path, "1,0"]);
    assert!(default_search.status.success());
    assert!(
        String::from_utf8(default_search.stdout)
            .unwrap()
            .contains("reason=SmallEligibleSet")
    );
    let before = std::fs::read(source.join("wal.bin")).unwrap();
    std::fs::write(&batch, "put\t9\t1,1\tvalid\nput\t10\t0,0\tinvalid\n").unwrap();
    assert!(!run(&["batch", path, file]).status.success());
    assert_eq!(before, std::fs::read(source.join("wal.bin")).unwrap());
    assert!(
        run(&["backup", path, backup.to_str().unwrap()])
            .status
            .success()
    );
    assert!(
        !run(&["backup", path, backup.to_str().unwrap()])
            .status
            .success()
    );
    assert!(run(&["maintain", path]).status.success());
    let stats = run(&["stats", path]);
    assert!(stats.status.success());
    assert!(
        String::from_utf8(stats.stdout)
            .unwrap()
            .contains("graph_cache_loaded=true")
    );
    let db = Database::open(&backup).unwrap();
    assert_eq!(db.config().metric, Metric::Cosine);
    assert_eq!(db.get(1).unwrap().vector, [2.0, 0.0]);
    assert_eq!(db.sequence(), 1);
    assert!(db.get(9).is_none());
}

#[test]
fn acknowledged_batch_survives_actual_process_kill_without_checkpoint() {
    let temp = TempDir::new();
    let source = temp.path().join("source");
    let batch = temp.path().join("batch.tsv");
    let path = source.to_str().unwrap();
    assert!(run(&["init", path, "2"]).status.success());
    std::fs::write(
        &batch,
        "put\t1\t1,2\toriginal\nput\t2\t3,4\ttemporary\nput\t1\t5,6\tupdated\ndelete\t2\n",
    )
    .unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_vecnook"))
        .args(["shell", path])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut child = ManagedChild(child);
    let stdout = child.0.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if sender.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(10)).unwrap(),
        "READY dimensions=2"
    );
    writeln!(child.0.stdin.as_mut().unwrap(), "batch {}", batch.display()).unwrap();
    child.0.stdin.as_mut().unwrap().flush().unwrap();
    let ack = receiver.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(ack.starts_with("OK batch sequence=1 inserted=2 updated=1 deleted=1 absent=0"));
    child.0.kill().unwrap();
    assert!(!child.0.wait().unwrap().success());
    reader.join().unwrap();
    let db = Database::open(&source).unwrap();
    assert_eq!(db.recovery_info().replayed_frames, 1);
    assert_eq!(db.get(1).unwrap().vector, [5.0, 6.0]);
    assert_eq!(db.get(1).unwrap().metadata, "updated");
    assert!(db.get(2).is_none());
}

#[test]
fn cli_file_benchmark_reads_real_files_and_rejects_invalid_metric() {
    let temp = TempDir::new();
    let corpus = temp.path().join("base.fvecs");
    let query = temp.path().join("query.fvecs");
    let encode = |vectors: &[[f32; 2]]| {
        let mut bytes = Vec::new();
        for vector in vectors {
            bytes.extend_from_slice(&2u32.to_le_bytes());
            for value in vector {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes
    };
    std::fs::write(&corpus, encode(&[[1.0, 0.0], [0.0, 1.0], [2.0, 0.0]])).unwrap();
    std::fs::write(&query, encode(&[[1.0, 0.1]])).unwrap();
    let args = [
        "bench-file",
        corpus.to_str().unwrap(),
        query.to_str().unwrap(),
        "3",
        "1",
        "128",
        "cosine",
    ];
    let result = run(&args);
    assert!(result.status.success());
    let output = String::from_utf8(result.stdout).unwrap();
    assert!(output.contains("dataset=fvecs") && output.contains("recall_at_3=100.0000%"));
    let mut invalid = args;
    invalid[6] = "bad-metric";
    assert!(!run(&invalid).status.success());
}
