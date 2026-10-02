mod support;
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    time::Duration,
};
use support::TempDir;
use vector::{Database, Error};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vector"))
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
    let child = Command::new(env!("CARGO_BIN_EXE_vector"))
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
    assert_eq!(db.recovery_info().replayed_operations, 4);
    assert_eq!(db.get(1).unwrap().vector, [5.0, 6.0]);
    assert_eq!(db.get(1).unwrap().metadata, "updated 한글");
    assert!(db.get(2).is_none());
}
