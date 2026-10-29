mod support;
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
    sync::{Mutex, mpsc},
    time::Duration,
};
use support::TempDir;
use vecnook::{
    Config, EmbeddingSpace, Metric,
    generation::{PublishOptions, Publisher},
};

static PROCESS_GATE: Mutex<()> = Mutex::new(());
struct Session {
    child: Child,
    lines: mpsc::Receiver<String>,
}
impl Session {
    fn open(store: &std::path::Path, extras: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_vecnook"))
            .arg("snapshot-session")
            .arg(store)
            .args(extras)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let output = child.stdout.take().unwrap();
        let (send, lines) = mpsc::sync_channel(4);
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                if send.send(line.unwrap()).is_err() {
                    break;
                }
            }
        });
        Self { child, lines }
    }
    fn line(&self) -> String {
        self.lines.recv_timeout(Duration::from_secs(5)).unwrap()
    }
    fn send(&mut self, text: &str) {
        self.child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn publish(publisher: &mut Publisher, sequence: u64, text: &str) -> u64 {
    let mut builder = publisher
        .begin(sequence, PublishOptions::default())
        .unwrap();
    builder.add(u64::MAX, &[1., -0.], text).unwrap();
    builder.add(1, &[0., 1.], "other").unwrap();
    builder.commit().unwrap().generation()
}
fn store(path: &std::path::Path) -> Publisher {
    Publisher::create(
        path,
        Config::new(2),
        EmbeddingSpace::new("session", 2, Metric::SquaredL2),
    )
    .unwrap()
}

#[test]
fn session_reuses_initialization_preserves_generation_and_releases_pin() {
    let _gate = PROCESS_GATE.lock().unwrap();
    let temp = TempDir::new();
    let path = temp.path().join("store");
    let mut publisher = store(&path);
    let generation = publish(&mut publisher, 7, "원문\nold");
    let mut session = Session::open(&path, &["--sq8", "16"]);
    let ready = session.line();
    assert!(
        ready.contains("\"snapshot_opens\":1") && ready.contains("\"sq8_builds\":1"),
        "{ready}"
    );
    session.send("{\"request_id\":\"a\",\"vector\":[1,-0.0],\"k\":1}\n");
    let first = session.line();
    assert!(
        first.contains("\"id\":\"18446744073709551615\"") && first.contains("\"request_id\":\"a\"")
    );
    assert!(first.contains("\"snapshot_opens\":1") && first.contains("\"sq8_builds\":1"));
    session.send("{\"request_id\":\"bad\",\"vector\":[1],\"k\":1}\n");
    assert!(session.line().contains("\"error\""));
    let next = publish(&mut publisher, 8, "new");
    assert_ne!(generation, next);
    publisher.collect_garbage().unwrap();
    assert!(path.join(format!("gen-{generation:016x}")).exists());
    session.send(
        "{\"request_id\":\"b\",\"vector\":[1,0],\"k\":1,\"ids\":[\"18446744073709551615\"]}\n",
    );
    let second = session.line();
    assert!(second.contains(&format!("\"generation\":\"{generation}\"")) && second.contains("old"));
    session.send("{\"request_id\":\"close\",\"op\":\"close\"}\n");
    assert!(session.line().contains("\"event\":\"closed\""));
    assert!(session.child.wait().unwrap().success());
    publisher.collect_garbage().unwrap();
    assert!(!path.join(format!("gen-{generation:016x}")).exists());
}

#[test]
fn batch_recovers_after_oversized_and_invalid_requests_and_closes_on_eof() {
    let _gate = PROCESS_GATE.lock().unwrap();
    let temp = TempDir::new();
    let path = temp.path().join("store");
    publish(&mut store(&path), 1, "original");
    let input = temp.path().join("batch.jsonl");
    let mut text = vec![b' '; 1024 * 1024 + 1];
    text.extend_from_slice(b"\n{\"request_id\":\"bad\",\"vector\":[NaN]}\n{\"request_id\":\"ok\",\"vector\":[1,0],\"k\":1}\n");
    fs::write(&input, text).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_vecnook"))
        .arg("snapshot-batch")
        .arg(&path)
        .arg(&input)
        .args(["--sq8", "16"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    let output = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<_> = output.lines().collect();
    assert_eq!(lines.len(), 5);
    assert!(lines[0].contains("\"event\":\"ready\""));
    assert!(lines[1].contains("exceeds 1 MiB") && lines[2].contains("\"error\""));
    assert!(lines[3].contains("\"request_id\":\"ok\"") && lines[3].contains("\"sq8_builds\":1"));
    assert!(lines[4].contains("\"reason\":\"eof\""));
}

#[test]
fn idle_eof_and_kill_release_reader_resources() {
    let _gate = PROCESS_GATE.lock().unwrap();
    let temp = TempDir::new();
    let path = temp.path().join("store");
    let mut publisher = store(&path);
    let old = publish(&mut publisher, 1, "old");
    let mut session = Session::open(&path, &["--idle-ms", "50"]);
    assert!(session.line().contains("\"event\":\"ready\""));
    assert!(session.line().contains("\"event\":\"idle_timeout\""));
    assert!(session.child.wait().unwrap().success());
    let mut session = Session::open(&path, &[]);
    assert!(session.line().contains("\"event\":\"ready\""));
    drop(session.child.stdin.take());
    assert!(session.line().contains("\"reason\":\"eof\""));
    assert!(session.child.wait().unwrap().success());
    let mut killed = Session::open(&path, &[]);
    assert!(killed.line().contains("\"event\":\"ready\""));
    publish(&mut publisher, 2, "new");
    publisher.collect_garbage().unwrap();
    assert!(path.join(format!("gen-{old:016x}")).exists());
    killed.child.kill().unwrap();
    killed.child.wait().unwrap();
    publisher.collect_garbage().unwrap();
    assert!(!path.join(format!("gen-{old:016x}")).exists());
}

#[test]
fn cached_cli_reuses_codes_and_refuses_pruning_live_sessions() {
    let _gate = PROCESS_GATE.lock().unwrap();
    let temp = TempDir::new();
    let path = temp.path().join("store");
    let cache = temp.path().join("cache");
    publish(&mut store(&path), 1, "원문");
    let binary = env!("CARGO_BIN_EXE_vecnook");
    let missing = Command::new(binary)
        .args(["--json-errors", "snapshot-search"])
        .arg(&path)
        .args(["1,0", "1", "--sq8", "16", "--sq8-cache"])
        .arg(&cache)
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(
        String::from_utf8(missing.stderr)
            .unwrap()
            .contains("snapshot-prepare-sq8")
    );
    assert!(!cache.exists(), "read commands must not create a cache");
    let prepared = Command::new(binary)
        .arg("snapshot-prepare-sq8")
        .arg(&path)
        .arg(&cache)
        .output()
        .unwrap();
    assert!(prepared.status.success(), "{:?}", prepared.stderr);
    let expected = Command::new(binary)
        .arg("snapshot-search")
        .arg(&path)
        .args(["1,0", "1", "--sq8", "16"])
        .output()
        .unwrap();
    let cached = Command::new(binary)
        .arg("snapshot-search")
        .arg(&path)
        .args(["1,0", "1", "--sq8", "16", "--sq8-cache"])
        .arg(&cache)
        .args(["--buffer-bytes", "36"])
        .output()
        .unwrap();
    assert!(cached.status.success(), "{:?}", cached.stderr);
    assert_eq!(expected.stdout, cached.stdout);
    let mut session = Session::open(
        &path,
        &[
            "--sq8",
            "16",
            "--sq8-cache",
            cache.to_str().unwrap(),
            "--buffer-bytes",
            "36",
        ],
    );
    let ready = session.line();
    assert!(
        ready.contains("\"sq8_builds\":0")
            && ready.contains("\"cache_hits\":1")
            && ready.contains("\"code_read_buffer_bytes\":36"),
        "{ready}"
    );
    let prune = Command::new(binary)
        .arg("--json-errors")
        .arg("snapshot-prune-sq8")
        .arg(&cache)
        .output()
        .unwrap();
    assert!(!prune.status.success());
    assert!(
        String::from_utf8(prune.stderr)
            .unwrap()
            .contains("\"kind\":\"Locked\"")
    );
    session.send("{\"request_id\":\"a\",\"vector\":[1,0],\"k\":1}\n");
    let response = session.line();
    assert!(response.contains("\"sq8_builds\":0") && response.contains("\"18446744073709551615\""));
    session.send("{\"request_id\":\"close\",\"op\":\"close\"}\n");
    assert!(session.line().contains("\"event\":\"closed\""));
    assert!(session.child.wait().unwrap().success());
    let prune = Command::new(binary)
        .arg("snapshot-prune-sq8")
        .arg(&cache)
        .output()
        .unwrap();
    assert!(prune.status.success(), "{:?}", prune.stderr);
    assert!(!cache.join("DATA").exists());
    assert!(cache.join("LEASE").exists() && path.join("CURRENT").exists());
}
