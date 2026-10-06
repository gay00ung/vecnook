mod support;
use std::{collections::BTreeMap, fs, io::Write, process::Command};
use support::TempDir;
use vecnook::{Collection, Config, Database, Document, Error, Metric, doctor};

fn bytes(path: &std::path::Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(path)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            // Windows enforces mandatory byte-range locking even for the owner's
            // second read handle. LOCK is deliberately empty; assert that without
            // reading a locked range, and still include it in the directory inventory.
            let contents = if e.file_name() == "LOCK" {
                assert_eq!(e.metadata().unwrap().len(), 0);
                Vec::new()
            } else {
                fs::read(e.path()).unwrap()
            };
            (e.file_name().to_str().unwrap().to_owned(), contents)
        })
        .collect()
}

#[test]
fn doctor_never_changes_clean_torn_corrupt_locked_or_missing_lock_storage() {
    let tmp = TempDir::new();
    let path = tmp.path().join("raw");
    let mut db = Database::create(&path, Config::new(2)).unwrap();
    db.put(u64::MAX, &[1.0, -0.0], "한글\n").unwrap();
    let locked = bytes(&path);
    assert!(matches!(doctor(&path), Err(Error::Locked)));
    assert_eq!(bytes(&path), locked);
    drop(db);
    fs::OpenOptions::new()
        .append(true)
        .open(path.join("wal.bin"))
        .unwrap()
        .write_all(&[1, 2, 3])
        .unwrap();
    let before = bytes(&path);
    let report = doctor(&path).unwrap();
    assert_eq!(report.pending_tail_bytes, 3);
    assert_eq!(report.capacity.active_records, 1);
    assert_eq!(report.capacity.vector_bytes, 8);
    assert_eq!(bytes(&path), before);
    let output = Command::new(env!("CARGO_BIN_EXE_vecnook"))
        .args(["doctor", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(bytes(&path), before);
    let mut corrupt = fs::read(path.join("wal.bin")).unwrap();
    corrupt[9] ^= 1;
    fs::write(path.join("wal.bin"), corrupt).unwrap();
    let bad = bytes(&path);
    assert!(matches!(doctor(&path), Err(Error::Corrupt(_))));
    assert_eq!(bytes(&path), bad);
    fs::remove_file(path.join("LOCK")).unwrap();
    let missing = bytes(&path);
    assert!(doctor(&path).is_err());
    assert_eq!(bytes(&path), missing);
}

#[test]
fn vector_export_preserves_bits_ids_metadata_and_exact_order_and_protects_destinations() {
    let tmp = TempDir::new();
    let mut db = Database::create(tmp.path().join("raw"), Config::new(2)).unwrap();
    for (id, v) in [
        (0, [0.0, -0.0]),
        (u64::MAX, [f32::from_bits(1), -12.5]),
        (7, [3.0, 4.0]),
    ] {
        db.put(id, &v, "한글\noriginal\r\n").unwrap();
    }
    db.put(7, &[5.0, 6.0], "updated").unwrap();
    db.delete(0).unwrap();
    let export = tmp.path().join("vectors.export");
    db.export(&export).unwrap();
    let content = fs::read(&export).unwrap();
    assert!(matches!(db.export(&export), Err(Error::AlreadyExists)));
    assert_eq!(fs::read(&export).unwrap(), content);
    let restored = Database::import(&export, tmp.path().join("restored")).unwrap();
    for a in db.iter() {
        let b = restored.get(a.id).unwrap();
        assert_eq!(a.metadata, b.metadata);
        assert_eq!(
            a.vector.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            b.vector.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        );
    }
    assert_eq!(
        restored.search_exact(&[0.0, 0.0], 10).unwrap().neighbors,
        db.search_exact(&[0.0, 0.0], 10).unwrap().neighbors
    );
    assert_eq!(restored.stats().tombstones, 0);
    let existing = bytes(&tmp.path().join("restored"));
    assert!(matches!(
        Database::import(&export, tmp.path().join("restored")),
        Err(Error::AlreadyExists)
    ));
    assert_eq!(bytes(&tmp.path().join("restored")), existing);
    for cut in 0..content.len() {
        let broken = tmp.path().join("broken.export");
        fs::write(&broken, &content[..cut]).unwrap();
        let destination = tmp.path().join("never-created");
        assert!(Database::import(&broken, &destination).is_err());
        assert!(!destination.exists());
    }
}

#[test]
fn document_export_backup_and_independent_restore_preserve_identity_originals_and_filters() {
    let tmp = TempDir::new();
    let mut c = Collection::create(
        tmp.path(),
        "notes",
        Config::new(2).with_metric(Metric::Cosine),
        "model@digest:prompt-v1",
    )
    .unwrap();
    let mut doc = Document::new(u64::MAX, "original\n한글\r\n", "source.md");
    doc.tags = vec!["rust".into()];
    doc.start_line = 8;
    doc.end_line = 9;
    c.put(&doc, &[1.0, 0.0]).unwrap();
    let space = c.space().clone();
    let archive = tmp.path().join("documents.export");
    c.export(&archive).unwrap();
    c.backup(tmp.path(), "backup").unwrap();
    let imported = Collection::import(&archive, tmp.path(), "imported").unwrap();
    assert_eq!(imported.space(), &space);
    assert_eq!(imported.get(u64::MAX).unwrap(), Some(doc.clone()));
    assert!(Database::import(&archive, tmp.path().join("wrong")).is_err());
    assert!(!tmp.path().join("wrong").exists());
    drop(imported);
    drop(c);
    for name in ["notes", "backup", "imported"] {
        let report = doctor(tmp.path().join(name)).unwrap();
        assert_eq!(report.space, Some(space.clone()));
        let before = bytes(&tmp.path().join(name));
        assert_eq!(
            doctor(tmp.path().join(name))
                .unwrap()
                .capacity
                .active_records,
            1
        );
        assert_eq!(bytes(&tmp.path().join(name)), before);
        let c = Collection::open(tmp.path(), name, &space).unwrap();
        assert_eq!(c.get(u64::MAX).unwrap(), Some(doc.clone()));
        c.check_invariants().unwrap();
    }
}
