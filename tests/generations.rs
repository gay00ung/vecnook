mod support;
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
    time::Duration,
};
use support::TempDir;
use vecnook::{
    Config, Database, EmbeddingSpace, ErrorKind, Metric,
    generation::{PublishOptions, Publisher, ScalarKernel, Snapshot},
};

fn create(path: &std::path::Path, metric: Metric, dimensions: usize) -> Publisher {
    Publisher::create(
        path,
        Config::new(dimensions).with_metric(metric),
        EmbeddingSpace::new("m", dimensions, metric),
    )
    .unwrap()
}
fn publish(publisher: &mut Publisher, sequence: u64, rows: &[(u64, Vec<f32>, String)]) -> Snapshot {
    let mut builder = publisher
        .begin(
            sequence,
            PublishOptions::default().with_segment_bytes(65536),
        )
        .unwrap();
    for (id, vector, metadata) in rows {
        builder.add(*id, vector, metadata).unwrap();
    }
    builder.commit().unwrap()
}
fn distance(metric: Metric, a: &[f32], b: &[f32]) -> f64 {
    let dot: f64 = a.iter().zip(b).map(|(&a, &b)| a as f64 * b as f64).sum();
    match metric {
        Metric::SquaredL2 => a
            .iter()
            .zip(b)
            .map(|(&a, &b)| {
                let d = a as f64 - b as f64;
                d * d
            })
            .sum(),
        Metric::InnerProduct => -dot,
        Metric::Cosine => {
            let a = a
                .iter()
                .map(|&v| (v as f64).powi(2))
                .sum::<f64>()
                .sqrt()
                .recip();
            let b = b
                .iter()
                .map(|&v| (v as f64).powi(2))
                .sum::<f64>()
                .sqrt()
                .recip();
            (1.0 - dot * a * b).clamp(0.0, 2.0)
        }
        _ => unreachable!(),
    }
}
#[test]
fn originals_exact_oracle_quantized_rerank_filters_and_backup() {
    let temp = TempDir::new();
    for (number, metric) in [Metric::SquaredL2, Metric::Cosine, Metric::InnerProduct]
        .into_iter()
        .enumerate()
    {
        let path = temp.path().join(number.to_string());
        let mut writer = create(&path, metric, 17);
        let mut rows: Vec<_> = (0..320u64)
            .map(|id| {
                (
                    id,
                    (0..17)
                        .map(|d| (((id * 31 + d * 13) % 211) as f32 - 100.0) / 31.0)
                        .collect(),
                    format!("원문 {id}"),
                )
            })
            .collect();
        rows.push((
            u64::MAX,
            vec![-0.0; 17]
                .into_iter()
                .enumerate()
                .map(|(n, v)| if n == 0 { 1.0 } else { v })
                .collect(),
            "original\n한글".into(),
        ));
        let snapshot = publish(&mut writer, 19, &rows);
        assert!(snapshot.segment_count() >= 1);
        assert_eq!(
            snapshot.get(u64::MAX).unwrap().unwrap().vector[1].to_bits(),
            (-0.0f32).to_bits()
        );
        let quantized = snapshot.quantize().unwrap();
        assert_eq!(quantized.code_bytes(), rows.len() * 17);
        assert_eq!(quantized.parameter_bytes(), rows.len() * 16);
        for query in [vec![1.0; 17], rows[55].1.clone(), rows[220].1.clone()] {
            let mut oracle: Vec<_> = rows
                .iter()
                .map(|(id, v, _)| (*id, distance(metric, &query, v)))
                .collect();
            oracle.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
            let exact = snapshot.search_exact(&query, 12).unwrap();
            assert_eq!(
                exact
                    .neighbors
                    .iter()
                    .map(|n| (n.id, n.distance.to_bits()))
                    .collect::<Vec<_>>(),
                oracle[..12]
                    .iter()
                    .map(|(id, d)| (*id, d.to_bits()))
                    .collect::<Vec<_>>()
            );
            let all = quantized.search(&query, 12, rows.len()).unwrap();
            assert_eq!(all.neighbors, exact.neighbors);
            assert!(!all.approximate);
            let approximate = quantized.search(&query, 12, 48).unwrap();
            assert_eq!(approximate.original_distance_computations, 48);
            assert!(approximate.complete && approximate.approximate);
            for n in &approximate.neighbors {
                let row = rows.iter().find(|row| row.0 == n.id).unwrap();
                assert_eq!(n.metadata, row.2);
                assert_eq!(
                    n.distance.to_bits(),
                    distance(metric, &query, &row.1).to_bits()
                );
            }
            let selected = quantized
                .search_ids(&query, 20, 32, &ScalarKernel, &[7, 7, 9, u64::MAX])
                .unwrap();
            assert_eq!(selected.eligible_count, 3);
            assert!(
                selected
                    .neighbors
                    .iter()
                    .all(|n| [7, 9, u64::MAX].contains(&n.id))
            );
            assert_eq!(
                snapshot.search_exact(&query, 0).unwrap().eligible_count,
                rows.len()
            );
            assert_eq!(
                quantized
                    .search(&query, 0, 0)
                    .unwrap()
                    .original_distance_computations,
                0
            );
        }
        let backup = snapshot
            .backup(
                temp.path().join(format!("backup-{number}")),
                PublishOptions::default(),
            )
            .unwrap();
        for (id, vector, metadata) in &rows {
            let copy = backup.get(*id).unwrap().unwrap();
            assert_eq!(copy.metadata, *metadata);
            assert_eq!(
                copy.vector.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
                vector.iter().map(|x| x.to_bits()).collect::<Vec<_>>()
            );
        }
        assert_eq!(backup.sequence(), 19);
        drop(writer);
        let mut writer = Publisher::open(&path).unwrap();
        assert_eq!(Snapshot::open(&path).unwrap().sequence(), 19);
        assert!(writer.begin(18, PublishOptions::default()).is_err());
    }
}

#[test]
fn high_dimension_replacements_deletions_reopen_pins_and_backup_preserve_originals() {
    let temp = TempDir::new();
    for dimensions in [768, 1536] {
        let path = temp.path().join(dimensions.to_string());
        let mut writer = create(&path, Metric::Cosine, dimensions);
        let mut pinned = Vec::new();
        for sequence in 1..=7u64 {
            let rows: Vec<_> = (0..300u64)
                .filter(|id| (id + sequence) % 7 != 0)
                .map(|id| {
                    let values = (0..dimensions)
                        .map(|d| {
                            if d == 0 {
                                -0.0
                            } else {
                                ((id * 31 + sequence * 13 + d as u64 * 7) % 199) as f32 / 17.0 - 5.0
                            }
                        })
                        .collect::<Vec<_>>();
                    (id, values, format!("original-{sequence}-{id}-원문"))
                })
                .collect();
            let current = publish(&mut writer, sequence, &rows);
            assert!(current.segment_count() > 1);
            let query = vec![0.125; dimensions];
            let mut oracle = rows
                .iter()
                .map(|(id, v, _)| (*id, distance(Metric::Cosine, &query, v)))
                .collect::<Vec<_>>();
            oracle.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
            let exact = current.search_exact(&query, 10).unwrap();
            assert_eq!(
                exact
                    .neighbors
                    .iter()
                    .map(|n| (n.id, n.distance.to_bits()))
                    .collect::<Vec<_>>(),
                oracle[..10]
                    .iter()
                    .map(|(id, d)| (*id, d.to_bits()))
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                current
                    .quantize()
                    .unwrap()
                    .search(&query, 10, current.len())
                    .unwrap()
                    .neighbors,
                exact.neighbors
            );
            for (id, expected, metadata) in &rows {
                let actual = current.get(*id).unwrap().unwrap();
                assert_eq!(actual.metadata, *metadata);
                assert!(
                    actual
                        .vector
                        .iter()
                        .zip(expected)
                        .all(|(a, b)| a.to_bits() == b.to_bits())
                );
            }
            pinned.push((current, rows));
            writer.collect_garbage().unwrap();
            drop(writer);
            writer = Publisher::open(&path).unwrap();
        }
        for (snapshot, rows) in &pinned {
            for (id, vector, metadata) in rows {
                let actual = snapshot.get(*id).unwrap().unwrap();
                assert_eq!(actual.metadata, *metadata);
                assert!(
                    actual
                        .vector
                        .iter()
                        .zip(vector)
                        .all(|(a, b)| a.to_bits() == b.to_bits())
                );
            }
        }
        let backup = pinned[0]
            .0
            .backup(
                temp.path().join(format!("backup-{dimensions}")),
                PublishOptions::default(),
            )
            .unwrap();
        assert_eq!(backup.sequence(), 1);
        assert_eq!(backup.len(), pinned[0].1.len());
        for (id, vector, metadata) in &pinned[0].1 {
            let restored = backup.get(*id).unwrap().unwrap();
            assert_eq!(restored.metadata, *metadata);
            assert!(
                restored
                    .vector
                    .iter()
                    .zip(vector)
                    .all(|(a, b)| a.to_bits() == b.to_bits())
            );
        }
        drop(pinned);
        assert_eq!(writer.collect_garbage().unwrap(), 6);
        assert_eq!(Snapshot::open(path).unwrap().sequence(), 7);
    }
}

#[test]
fn immutable_readers_survive_publication_and_gc_only_collects_unpinned_generations() {
    let temp = TempDir::new();
    let path = temp.path().join("store");
    let mut writer = create(&path, Metric::SquaredL2, 2);
    let old = publish(&mut writer, 1, &[(7, vec![1.0, 2.0], "old".into())]);
    assert_eq!(
        Publisher::open(&path).err().unwrap().kind(),
        ErrorKind::Locked
    );
    let second = Snapshot::open(&path).unwrap();
    let current = publish(&mut writer, 2, &[(7, vec![3.0, 4.0], "new".into())]);
    assert_eq!(old.get(7).unwrap().unwrap().metadata, "old");
    assert_eq!(second.sequence(), 1);
    assert_eq!(old.refresh().unwrap().sequence(), 2);
    writer.collect_garbage().unwrap();
    assert!(path.join(format!("gen-{:016x}", old.generation())).exists());
    drop(old);
    drop(second);
    assert_eq!(writer.collect_garbage().unwrap(), 1);
    assert_eq!(current.get(7).unwrap().unwrap().metadata, "new");
}

#[test]
fn invalid_duplicate_partial_input_cannot_publish_and_old_database_remains_compatible() {
    let temp = TempDir::new();
    let path = temp.path().join("store");
    let mut writer = create(&path, Metric::SquaredL2, 2);
    let old = Snapshot::open(&path).unwrap().generation();
    let mut builder = writer.begin(1, PublishOptions::default()).unwrap();
    builder.add(1, &[1., 2.], "one").unwrap();
    assert!(builder.add(1, &[3., 4.], "duplicate").is_err());
    assert!(builder.commit().is_err());
    assert_eq!(Snapshot::open(&path).unwrap().generation(), old);
    let mut builder = writer.begin(1, PublishOptions::default()).unwrap();
    assert!(builder.add(2, &[f32::NAN, 2.], "bad").is_err());
    assert!(builder.commit().is_err());
    let dbpath = temp.path().join("legacy");
    let mut db = Database::create(&dbpath, Config::new(2)).unwrap();
    db.put(u64::MAX, &[f32::MAX, f32::from_bits(1)], "original")
        .unwrap();
    let published = db
        .publish_generation(&mut writer, PublishOptions::default())
        .unwrap();
    assert_eq!(published.sequence(), db.sequence());
    assert_eq!(
        published.get(u64::MAX).unwrap().unwrap(),
        *db.get(u64::MAX).unwrap()
    );
    db.checkpoint().unwrap();
    drop(db);
    assert_eq!(
        Database::open(&dbpath)
            .unwrap()
            .get(u64::MAX)
            .unwrap()
            .vector,
        [f32::MAX, f32::from_bits(1)]
    );
    assert!(Database::open(&path).is_err());
    assert_eq!(
        Publisher::create(
            &path,
            Config::new(2),
            EmbeddingSpace::new("m", 2, Metric::SquaredL2)
        )
        .err()
        .unwrap()
        .kind(),
        ErrorKind::AlreadyExists
    );
}

#[test]
fn corrupted_pointer_manifest_and_segment_fail_closed() {
    for file in ["CURRENT", "MANIFEST", "segment-0000.bin"] {
        let temp = TempDir::new();
        let path = temp.path().join("store");
        let mut writer = create(&path, Metric::SquaredL2, 2);
        let snapshot = publish(
            &mut writer,
            1,
            &[(u64::MAX, vec![1., -0.], "original".into())],
        );
        let target = if file == "CURRENT" {
            path.join(file)
        } else {
            path.join(format!("gen-{:016x}", snapshot.generation()))
                .join(file)
        };
        let mut bytes = fs::read(&target).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        fs::write(target, bytes).unwrap();
        assert_eq!(
            Snapshot::open(&path).err().unwrap().kind(),
            ErrorKind::Corrupt
        );
    }
}

#[test]
#[ignore = "subprocess process-reader worker"]
fn reader_worker() {
    let snapshot = Snapshot::open(std::env::var_os("VECNOOK_GENERATION").unwrap()).unwrap();
    println!("PINNED {}", snapshot.generation());
    std::io::stdout().flush().unwrap();
    std::io::stdin().read_exact(&mut [0]).unwrap();
    assert_eq!(snapshot.get(7).unwrap().unwrap().metadata, "old");
    println!("STILL_OLD");
    std::io::stdout().flush().unwrap();
    std::io::stdin().read_exact(&mut [0]).unwrap();
}

#[test]
fn separate_process_pin_is_released_by_process_exit() {
    let temp = TempDir::new();
    let path = temp.path().join("store");
    let mut writer = create(&path, Metric::SquaredL2, 2);
    let old = publish(&mut writer, 1, &[(7, vec![1., 2.], "old".into())]);
    let old_id = old.generation();
    drop(old);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "reader_worker", "--nocapture"])
        .env("VECNOOK_GENERATION", &path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if send.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    loop {
        if receive
            .recv_timeout(Duration::from_secs(20))
            .unwrap()
            .starts_with("PINNED ")
        {
            break;
        }
    }
    let current = publish(&mut writer, 2, &[(7, vec![3., 4.], "new".into())]);
    writer.collect_garbage().unwrap();
    assert!(path.join(format!("gen-{old_id:016x}")).exists());
    child.stdin.as_mut().unwrap().write_all(b"!").unwrap();
    loop {
        if receive.recv_timeout(Duration::from_secs(20)).unwrap() == "STILL_OLD" {
            break;
        }
    }
    child.kill().unwrap();
    assert!(!child.wait().unwrap().success());
    reader.join().unwrap();
    assert_eq!(writer.collect_garbage().unwrap(), 1);
    assert_eq!(current.get(7).unwrap().unwrap().metadata, "new");
}

#[test]
#[ignore = "subprocess interrupted publisher"]
fn publisher_worker() {
    let path = std::env::var_os("VECNOOK_GENERATION").unwrap();
    let mut writer = Publisher::open(path).unwrap();
    let mut builder = writer
        .begin(2, PublishOptions::default().with_segment_bytes(65536))
        .unwrap();
    let metadata = "original".repeat(1000);
    for id in 0..4096 {
        builder.add(id, &[id as f32, -0.0], &metadata).unwrap();
    }
    println!("PREPARED");
    std::io::stdout().flush().unwrap();
    std::io::stdin().read_exact(&mut [0]).unwrap();
    builder.commit().unwrap();
    println!("COMMITTED");
    std::io::stdout().flush().unwrap();
    std::io::stdin().read_exact(&mut [0]).unwrap();
}

#[test]
fn killed_publisher_never_exposes_partial_segments_or_loses_a_publication_ack() {
    let temp = TempDir::new();
    let path = temp.path().join("store");
    let mut writer = create(&path, Metric::SquaredL2, 2);
    drop(publish(&mut writer, 1, &[(7, vec![1., 2.], "old".into())]));
    drop(writer);
    for commit in [false, true] {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "publisher_worker", "--nocapture"])
            .env("VECNOOK_GENERATION", &path)
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
            if receive.recv_timeout(Duration::from_secs(30)).unwrap() == "PREPARED" {
                break;
            }
        }
        let mut acknowledged = false;
        if commit {
            child.stdin.as_mut().unwrap().write_all(b"!").unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            loop {
                if receive.try_iter().any(|line| line == "COMMITTED") {
                    acknowledged = true;
                    break;
                }
                if path.join("CURRENT.tmp").exists() {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "publisher bounded state wait"
                );
                std::thread::yield_now();
            }
        }
        child.kill().unwrap();
        assert!(!child.wait().unwrap().success());
        reader.join().unwrap();
        acknowledged |= receive.try_iter().any(|line| line == "COMMITTED");
        let snapshot = Snapshot::open(&path).unwrap();
        if acknowledged {
            assert_eq!(snapshot.sequence(), 2);
        }
        match snapshot.sequence() {
            1 => {
                assert!(!acknowledged);
                assert_eq!(snapshot.len(), 1);
                assert_eq!(snapshot.get(7).unwrap().unwrap().metadata, "old");
            }
            2 => {
                assert!(commit);
                assert_eq!(snapshot.len(), 4096);
                for id in [0, 7, 4095] {
                    let r = snapshot.get(id).unwrap().unwrap();
                    assert_eq!(r.vector[0], id as f32);
                    assert_eq!(r.vector[1].to_bits(), (-0.0f32).to_bits());
                    assert_eq!(r.metadata, "original".repeat(1000));
                }
            }
            other => panic!("mixed or unexpected generation sequence {other}"),
        }
    }
}

#[test]
fn native_snapshot_cli_roundtrips_originals_and_rejects_identity_changes() {
    let temp = TempDir::new();
    let source = temp.path().join("db");
    let store = temp.path().join("store");
    let backup = temp.path().join("backup");
    let mut db = Database::create(&source, Config::new(2)).unwrap();
    db.put(u64::MAX, &[1., -0.], "original\n원문").unwrap();
    drop(db);
    let binary = env!("CARGO_BIN_EXE_vecnook");
    let run = |arguments: &[&str]| Command::new(binary).args(arguments).output().unwrap();
    let (source, store, backup) = (
        source.to_str().unwrap(),
        store.to_str().unwrap(),
        backup.to_str().unwrap(),
    );
    assert!(
        run(&["snapshot-publish", source, store, "model"])
            .status
            .success()
    );
    let get = run(&["snapshot-get", store, &u64::MAX.to_string()]);
    assert!(get.status.success());
    let text = String::from_utf8(get.stdout).unwrap();
    assert!(
        text.contains("\"18446744073709551615\"") && text.contains("-0.0") && text.contains("원문")
    );
    assert!(
        run(&["snapshot-search", store, "1,0", "1", "--sq8", "16"])
            .status
            .success()
    );
    assert!(run(&["snapshot-backup", store, backup]).status.success());
    assert!(run(&["snapshot-info", backup]).status.success());
    assert!(
        !run(&["snapshot-publish", source, store, "changed-model"])
            .status
            .success()
    );
    assert!(
        !run(&["snapshot-search", store, "nan,0", "1"])
            .status
            .success()
    );
    assert!(run(&["snapshot-gc", store]).status.success());
}

#[test]
fn collection_publication_binds_model_and_original_document_payload() {
    let temp = TempDir::new();
    let mut collection =
        vecnook::Collection::create(temp.path(), "documents", Config::new(2), "model-v1").unwrap();
    let document = vecnook::Document::new(u64::MAX, "원문 보존", "한글.md");
    collection.put(&document, &[1., -0.]).unwrap();
    let mut publisher = Publisher::create(
        temp.path().join("store"),
        Config::new(2),
        collection.space().clone(),
    )
    .unwrap();
    let snapshot = collection
        .publish_generation(&mut publisher, PublishOptions::default())
        .unwrap();
    let original = snapshot.get(u64::MAX).unwrap().unwrap();
    assert_eq!(
        vecnook::Document::from_payload(&original.metadata).unwrap(),
        document
    );
    assert_eq!(original.vector[1].to_bits(), (-0.0f32).to_bits());
    let mut wrong = Publisher::create(
        temp.path().join("wrong"),
        Config::new(2),
        EmbeddingSpace::new("other", 2, Metric::SquaredL2),
    )
    .unwrap();
    assert_eq!(
        collection
            .publish_generation(&mut wrong, PublishOptions::default())
            .err()
            .unwrap()
            .kind(),
        ErrorKind::EmbeddingMismatch
    );
}
