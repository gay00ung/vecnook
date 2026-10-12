use crate::{
    Config, Database, Error, Mutation,
    storage_io::faults::{Guard, Mode},
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static COUNTER: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "vecnook-fault-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
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
fn short_and_interrupted_wal_writes_retry_without_losing_the_acknowledged_batch() {
    for mode in [Mode::Short, Mode::Interrupted] {
        let tmp = Temp::new();
        let mut db = Database::create(&tmp.0, Config::new(1)).unwrap();
        let guard = Guard::new("wal.write", mode);
        db.write_batch(&[
            Mutation::Put {
                id: 7,
                vector: &[7.0],
                metadata: "original",
            },
            Mutation::Put {
                id: 8,
                vector: &[8.0],
                metadata: "other",
            },
        ])
        .unwrap();
        drop(guard);
        drop(db);
        let db = Database::open(&tmp.0).unwrap();
        assert_eq!(db.len(), 2);
        assert_eq!(db.sequence(), 1);
    }
}

#[test]
fn every_partial_batch_write_preserves_old_acknowledgements_and_never_replays_partial_members() {
    let operations = [
        Mutation::Delete { id: 7 },
        Mutation::Put {
            id: 8,
            vector: &[8.0],
            metadata: "new",
        },
    ];
    let size = crate::storage::encode_batch(2, &operations).len();
    for cut in 0..size {
        let tmp = Temp::new();
        let mut db = Database::create(&tmp.0, Config::new(1)).unwrap();
        db.put(7, &[7.0], "acknowledged").unwrap();
        let guard = Guard::new("wal.write", Mode::Partial(cut));
        assert!(matches!(db.write_batch(&operations), Err(Error::Io(_))));
        assert_eq!(db.get(7).unwrap().metadata, "acknowledged");
        assert!(db.get(8).is_none());
        assert!(matches!(db.put(9, &[9.0], "blocked"), Err(Error::Poisoned)));
        assert!(matches!(
            db.export(tmp.0.join("ambiguous.export")),
            Err(Error::Poisoned)
        ));
        drop(guard);
        drop(db);
        let recovered = Database::open(&tmp.0).unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered.get(7).unwrap().metadata, "acknowledged");
        assert!(recovered.get(8).is_none());
        assert_eq!(recovered.sequence(), 1);
    }
}

#[test]
fn wal_sync_failure_is_ambiguous_but_recovers_a_whole_batch_and_poison_blocks_retry() {
    let tmp = Temp::new();
    let mut db = Database::create(&tmp.0, Config::new(1)).unwrap();
    db.put(7, &[7.0], "acknowledged").unwrap();
    let guard = Guard::new("wal.sync", Mode::Fail);
    assert!(matches!(
        db.write_batch(&[
            Mutation::Delete { id: 7 },
            Mutation::Put {
                id: 8,
                vector: &[8.0],
                metadata: "new"
            }
        ]),
        Err(Error::Io(_))
    ));
    assert_eq!(db.len(), 1);
    assert!(db.get(7).is_some());
    assert!(matches!(db.delete(7), Err(Error::Poisoned)));
    drop(guard);
    drop(db);
    let db = Database::open(&tmp.0).unwrap();
    assert!(db.get(7).is_none());
    assert_eq!(db.get(8).unwrap().metadata, "new");
    assert_eq!(db.sequence(), 2);
}

#[test]
fn checkpoint_and_compaction_failure_points_preserve_all_acknowledged_records() {
    for compact in [false, true] {
        for point in [
            "snapshot.write",
            "snapshot.sync",
            "snapshot.rename",
            "directory.sync",
            "cache.write",
            "cache.sync",
            "cache.rename",
            "wal.truncate",
            "wal.clear_sync",
        ] {
            let tmp = Temp::new();
            let mut db = Database::create(&tmp.0, Config::new(1)).unwrap();
            db.put(1, &[1.0], "first").unwrap();
            db.put(2, &[2.0], "deleted").unwrap();
            db.checkpoint().unwrap();
            db.put(1, &[3.0], "acknowledged update").unwrap();
            db.delete(2).unwrap();
            db.put(3, &[4.0], "acknowledged insert").unwrap();
            let guard = Guard::new(
                point,
                if point.ends_with("write") {
                    Mode::Partial(13)
                } else {
                    Mode::Fail
                },
            );
            let result = if compact {
                db.compact().map(|_| ())
            } else {
                db.checkpoint()
            };
            assert!(
                matches!(result, Err(Error::Io(_))),
                "point={point} compact={compact}"
            );
            assert!(matches!(
                db.put(99, &[99.0], "blocked"),
                Err(Error::Poisoned)
            ));
            drop(guard);
            drop(db);
            let db = Database::open(&tmp.0).unwrap();
            assert_eq!(db.len(), 2, "{point}");
            assert_eq!(db.get(1).unwrap().vector, [3.0]);
            assert_eq!(db.get(1).unwrap().metadata, "acknowledged update");
            assert!(db.get(2).is_none());
            assert_eq!(db.get(3).unwrap().metadata, "acknowledged insert");
            db.check_invariants().unwrap();
        }
    }
}

#[test]
fn failed_backup_copy_and_export_leave_source_intact_and_invalid_artifacts_are_rejected() {
    for point in ["backup.copy", "backup.sync", "export.write", "export.sync"] {
        let tmp = Temp::new();
        let mut db = Database::create(tmp.0.join("source"), Config::new(1)).unwrap();
        db.put(7, &[7.0], "acknowledged").unwrap();
        let guard = Guard::new(
            point,
            if point == "backup.copy" || point == "export.write" {
                Mode::Partial(9)
            } else {
                Mode::Fail
            },
        );
        if point.starts_with("backup") {
            assert!(db.backup(tmp.0.join("backup")).is_err());
            assert!(Database::open(tmp.0.join("backup")).is_err());
        } else {
            assert!(db.export(tmp.0.join("export")).is_err());
            if point == "export.write" {
                assert!(Database::import(tmp.0.join("export"), tmp.0.join("import")).is_err());
                assert!(!tmp.0.join("import").exists());
            }
        }
        drop(guard);
        assert_eq!(db.get(7).unwrap().metadata, "acknowledged");
        drop(db);
        assert_eq!(
            Database::open(tmp.0.join("source"))
                .unwrap()
                .get(7)
                .unwrap()
                .metadata,
            "acknowledged"
        );
    }
}
