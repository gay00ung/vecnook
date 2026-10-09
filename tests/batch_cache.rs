mod support;
use std::fs;
use support::TempDir;
use vecnook::{Config, Database, Error, MaintenanceAction, MaintenancePolicy, Metric, Mutation};

fn crc(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for &byte in bytes {
        c ^= u32::from(byte);
        for _ in 0..8 {
            c = (c >> 1) ^ if c & 1 != 0 { 0xedb88320 } else { 0 };
        }
    }
    !c
}
fn seal(bytes: &mut [u8]) {
    let end = bytes.len() - 4;
    let c = crc(&bytes[..end]);
    bytes[end..].copy_from_slice(&c.to_le_bytes());
}
fn seal_frame(bytes: &mut [u8]) {
    let end = bytes.len() - 4;
    let c = crc(&bytes[8..end]);
    bytes[end..].copy_from_slice(&c.to_le_bytes());
}

#[test]
fn ordered_mixed_batch_has_one_sequence_and_recovers_all_effects() {
    let temp = TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(1)).unwrap();
    db.put(1, &[1.0], "one").unwrap();
    db.put(2, &[2.0], "old").unwrap();
    let report = db
        .write_batch(&[
            Mutation::Put {
                id: 3,
                vector: &[3.0],
                metadata: "初 🐈",
            },
            Mutation::Put {
                id: 3,
                vector: &[4.0],
                metadata: "updated",
            },
            Mutation::Delete { id: 2 },
            Mutation::Delete { id: 99 },
            Mutation::Put {
                id: 2,
                vector: &[8.0],
                metadata: "restored",
            },
            Mutation::Delete { id: 3 },
        ])
        .unwrap();
    assert_eq!(
        (
            report.sequence,
            report.inserted,
            report.updated,
            report.deleted,
            report.absent
        ),
        (3, 2, 1, 2, 1)
    );
    assert_eq!(db.stats().physical_nodes, 5);
    drop(db);
    let db = Database::open(temp.path()).unwrap();
    assert_eq!(db.sequence(), 3);
    assert_eq!(db.recovery_info().replayed_frames, 3);
    assert_eq!(db.get(2).unwrap().vector, [8.0]);
    assert!(db.get(3).is_none());
    db.check_invariants().unwrap();
}

#[test]
fn invalid_last_batch_member_never_changes_ram_wal_or_sequence() {
    for metric in [Metric::SquaredL2, Metric::Cosine, Metric::InnerProduct] {
        let temp = TempDir::new();
        let mut db = Database::create(temp.path(), Config::new(2).with_metric(metric)).unwrap();
        db.put(1, &[1.0, 1.0], "old").unwrap();
        let wal = fs::read(temp.path().join("wal.bin")).unwrap();
        for invalid in [&[f32::NAN, 1.0][..], &[1.0][..]] {
            assert!(
                db.write_batch(&[
                    Mutation::Delete { id: 1 },
                    Mutation::Put {
                        id: 2,
                        vector: invalid,
                        metadata: "bad"
                    }
                ])
                .is_err()
            );
            assert_eq!(db.get(1).unwrap().metadata, "old");
            assert_eq!(db.sequence(), 1);
            assert_eq!(fs::read(temp.path().join("wal.bin")).unwrap(), wal);
        }
        if metric == Metric::Cosine {
            assert!(
                db.write_batch(&[
                    Mutation::Delete { id: 1 },
                    Mutation::Put {
                        id: 2,
                        vector: &[0.0, 0.0],
                        metadata: ""
                    }
                ])
                .is_err()
            );
        }
    }
}

#[test]
fn empty_and_absent_delete_batches_do_not_append_or_advance_sequence() {
    let temp = TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(1)).unwrap();
    assert_eq!(db.write_batch(&[]).unwrap().sequence, 0);
    let report = db
        .write_batch(&[Mutation::Delete { id: 1 }, Mutation::Delete { id: 1 }])
        .unwrap();
    assert_eq!((report.sequence, report.absent), (0, 2));
    assert_eq!(fs::metadata(temp.path().join("wal.bin")).unwrap().len(), 0);
}

#[test]
fn oversized_batch_count_and_payload_fail_before_writing() {
    let temp = TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(1)).unwrap();
    assert!(
        db.write_batch(&vec![Mutation::Delete { id: 1 }; 1025])
            .is_err()
    );
    let metadata = "x".repeat(16384);
    let operations = vec![
        Mutation::Put {
            id: 1,
            vector: &[1.0],
            metadata: &metadata
        };
        513
    ];
    assert!(db.write_batch(&operations).is_err());
    assert!(db.get(1).is_none());
    assert_eq!(fs::metadata(temp.path().join("wal.bin")).unwrap().len(), 0);
}

#[test]
fn maximum_operation_batch_can_checkpoint_with_count_above_sequence() {
    let temp = TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(1)).unwrap();
    let coordinate = [1.0];
    let operations: Vec<_> = (0..1024)
        .map(|id| Mutation::Put {
            id,
            vector: &coordinate,
            metadata: "",
        })
        .collect();
    assert_eq!(db.write_batch(&operations).unwrap().inserted, 1024);
    assert_eq!(db.sequence(), 1);
    db.checkpoint().unwrap();
    drop(db);
    let db = Database::open(temp.path()).unwrap();
    assert_eq!(db.stats().active_records, 1024);
    assert!(db.get(1023).is_some());
    assert!(db.recovery_info().graph_cache_loaded);
}

#[test]
fn torn_batch_at_every_byte_boundary_recovers_none_of_its_members() {
    let source = TempDir::new();
    let mut db = Database::create(source.path(), Config::new(2)).unwrap();
    db.put(1, &[1.0, 2.0], "original").unwrap();
    let prefix = fs::metadata(source.path().join("wal.bin")).unwrap().len() as usize;
    db.write_batch(&[
        Mutation::Delete { id: 1 },
        Mutation::Put {
            id: 2,
            vector: &[3.0, 4.0],
            metadata: "새로운 🐈",
        },
        Mutation::Put {
            id: 3,
            vector: &[5.0, 6.0],
            metadata: "third",
        },
    ])
    .unwrap();
    drop(db);
    let snapshot = fs::read(source.path().join("snapshot.bin")).unwrap();
    let wal = fs::read(source.path().join("wal.bin")).unwrap();
    for cut in prefix..wal.len() {
        let target = TempDir::new();
        fs::write(target.path().join("snapshot.bin"), &snapshot).unwrap();
        fs::write(target.path().join("wal.bin"), &wal[..cut]).unwrap();
        let mut recovered = Database::open(target.path()).unwrap();
        assert_eq!(recovered.get(1).unwrap().metadata, "original");
        assert!(recovered.get(2).is_none() && recovered.get(3).is_none());
        assert_eq!(recovered.sequence(), 1);
        assert_eq!(
            recovered.recovery_info().truncated_tail_bytes,
            (cut - prefix) as u64
        );
        recovered.put(4, &[7.0, 8.0], "after").unwrap();
        drop(recovered);
        assert!(Database::open(target.path()).unwrap().get(4).is_some());
    }
}

#[test]
fn complete_corrupt_batch_fails_without_truncating_or_masking_damage() {
    let temp = TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(1)).unwrap();
    db.write_batch(&[Mutation::Put {
        id: 1,
        vector: &[1.0],
        metadata: "one",
    }])
    .unwrap();
    drop(db);
    let path = temp.path().join("wal.bin");
    let original = fs::read(&path).unwrap();
    for count in [0u32, 1025, u32::MAX] {
        let mut bad = original.clone();
        bad[17..21].copy_from_slice(&count.to_le_bytes());
        seal_frame(&mut bad);
        fs::write(&path, &bad).unwrap();
        assert!(matches!(
            Database::open(temp.path()),
            Err(Error::Corrupt(_))
        ));
        assert_eq!(fs::read(&path).unwrap(), bad);
    }
    let mut bad = original;
    bad[21] = 255;
    seal_frame(&mut bad);
    fs::write(&path, &bad).unwrap();
    assert!(matches!(
        Database::open(temp.path()),
        Err(Error::Corrupt(_))
    ));
    assert_eq!(fs::read(path).unwrap(), bad);
}

#[test]
fn cache_restores_graph_then_applies_updates_deletes_and_new_nodes() {
    for metric in [Metric::SquaredL2, Metric::Cosine, Metric::InnerProduct] {
        let temp = TempDir::new();
        let mut db = Database::create(temp.path(), Config::new(3).with_metric(metric)).unwrap();
        for id in 0..180 {
            db.put(id, &[(id % 17) as f32, (id % 31) as f32, 1.0], "base")
                .unwrap();
        }
        db.checkpoint().unwrap();
        db.put(50, &[1.0, 2.0, 3.0], "updated").unwrap();
        db.delete(70).unwrap();
        db.put(181, &[3.0, 2.0, 1.0], "new").unwrap();
        let before = db.search_hnsw(&[2.0, 3.0, 1.0], 20, 512).unwrap();
        drop(db);
        let mut db = Database::open(temp.path()).unwrap();
        assert!(db.recovery_info().graph_cache_loaded);
        assert_eq!(db.recovery_info().cached_nodes, 180);
        assert_eq!(db.recovery_info().replayed_frames, 3);
        assert!(db.get(70).is_none());
        assert_eq!(db.get(50).unwrap().metadata, "updated");
        let after = db.search_hnsw(&[2.0, 3.0, 1.0], 20, 512).unwrap();
        assert_eq!(before.neighbors, after.neighbors);
        assert_eq!(before.distance_computations, after.distance_computations);
        db.put(182, &[4.0, 3.0, 2.0], "post-open").unwrap();
        db.check_invariants().unwrap();
    }
}

#[test]
fn cache_missing_stale_and_checksum_damage_rebuild_without_data_loss() {
    let temp = TempDir::new();
    let other = TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(1)).unwrap();
    db.put(1, &[1.0], "keep").unwrap();
    db.checkpoint().unwrap();
    drop(db);
    let good = fs::read(temp.path().join("index.bin")).unwrap();
    let mut different = Database::create(other.path(), Config::new(1)).unwrap();
    different.put(1, &[2.0], "different").unwrap();
    different.checkpoint().unwrap();
    drop(different);
    let stale = fs::read(other.path().join("index.bin")).unwrap();
    let mut corrupt = good.clone();
    corrupt[20] ^= 1;
    for cache in [None, Some(stale), Some(corrupt)] {
        match cache {
            None => fs::remove_file(temp.path().join("index.bin")).unwrap(),
            Some(bytes) => fs::write(temp.path().join("index.bin"), bytes).unwrap(),
        }
        let db = Database::open(temp.path()).unwrap();
        assert!(!db.recovery_info().graph_cache_loaded);
        assert!(db.recovery_info().graph_cache_note.is_some());
        assert_eq!(db.get(1).unwrap().metadata, "keep");
        drop(db);
        fs::write(temp.path().join("index.bin"), &good).unwrap();
    }
}

#[test]
fn valid_crc_but_invalid_cache_bounds_fail_safe_without_panics() {
    let temp = TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(1)).unwrap();
    for id in 0..3 {
        db.put(id, &[id as f32], "keep").unwrap();
    }
    db.checkpoint().unwrap();
    drop(db);
    let original = fs::read(temp.path().join("index.bin")).unwrap();
    for (offset, value) in [
        (24, u64::MAX.to_le_bytes().to_vec()),
        (32, u64::MAX.to_le_bytes().to_vec()),
        (40, 0u64.to_le_bytes().to_vec()),
        (48, vec![0]),
        (49, u32::MAX.to_le_bytes().to_vec()),
        (53, u32::MAX.to_le_bytes().to_vec()),
    ] {
        let mut bad = original.clone();
        bad[offset..offset + value.len()].copy_from_slice(&value);
        seal(&mut bad);
        fs::write(temp.path().join("index.bin"), bad).unwrap();
        let db = Database::open(temp.path()).unwrap();
        assert!(!db.recovery_info().graph_cache_loaded);
        assert_eq!(db.stats().active_records, 3);
        db.check_invariants().unwrap();
    }
}

#[test]
fn every_truncated_cache_falls_back_to_authoritative_records() {
    let temp = TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(1)).unwrap();
    db.put(1, &[1.0], "keep").unwrap();
    db.checkpoint().unwrap();
    drop(db);
    let original = fs::read(temp.path().join("index.bin")).unwrap();
    for cut in 0..original.len() {
        fs::write(temp.path().join("index.bin"), &original[..cut]).unwrap();
        let db = Database::open(temp.path()).unwrap();
        assert!(!db.recovery_info().graph_cache_loaded);
        assert!(db.get(1).is_some());
    }
}

#[test]
fn leftover_batch_wal_after_checkpoint_is_not_reapplied() {
    let temp = TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(1)).unwrap();
    db.write_batch(&[
        Mutation::Put {
            id: 1,
            vector: &[1.0],
            metadata: "a",
        },
        Mutation::Put {
            id: 2,
            vector: &[2.0],
            metadata: "b",
        },
    ])
    .unwrap();
    let old = fs::read(temp.path().join("wal.bin")).unwrap();
    db.checkpoint().unwrap();
    db.put(3, &[3.0], "c").unwrap();
    let new = fs::read(temp.path().join("wal.bin")).unwrap();
    drop(db);
    fs::write(temp.path().join("wal.bin"), [old, new].concat()).unwrap();
    let db = Database::open(temp.path()).unwrap();
    assert_eq!(db.stats().physical_nodes, 3);
    assert_eq!(db.recovery_info().skipped_frames, 1);
    assert_eq!(db.recovery_info().replayed_frames, 1);
    assert!(db.recovery_info().graph_cache_loaded);
}

#[test]
fn backup_is_independent_preserves_metric_and_never_overwrites() {
    let source = TempDir::new();
    let target = TempDir::new();
    let backup = target.path().join("backup");
    let mut db =
        Database::create(source.path(), Config::new(2).with_metric(Metric::Cosine)).unwrap();
    db.put(1, &[3.0, 4.0], "before").unwrap();
    db.backup(&backup).unwrap();
    assert!(matches!(db.backup(&backup), Err(Error::AlreadyExists)));
    assert!(matches!(
        db.backup(source.path()),
        Err(Error::AlreadyExists)
    ));
    db.put(1, &[1.0, 1.0], "after").unwrap();
    let mut restored = Database::open(&backup).unwrap();
    assert_eq!(restored.config().metric, Metric::Cosine);
    assert_eq!(restored.get(1).unwrap().metadata, "before");
    assert!(restored.recovery_info().graph_cache_loaded);
    restored.put(2, &[1.0, 2.0], "independent").unwrap();
    assert!(db.get(2).is_none());
}

#[test]
fn backup_rejects_an_existing_empty_directory_without_mutating_source() {
    let source = TempDir::new();
    let target = TempDir::new();
    let mut db = Database::create(source.path(), Config::new(1)).unwrap();
    db.put(1, &[1.0], "keep").unwrap();
    let wal = fs::read(source.path().join("wal.bin")).unwrap();
    assert!(matches!(
        db.backup(target.path()),
        Err(Error::AlreadyExists)
    ));
    assert_eq!(fs::read(source.path().join("wal.bin")).unwrap(), wal);
}

#[test]
fn maintenance_compacts_churn_or_checkpoints_wal_and_leaves_invalid_policy_unchanged() {
    let temp = TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(1)).unwrap();
    let policy = MaintenancePolicy::default()
        .with_wal_bytes(1)
        .with_min_tombstones(2)
        .with_tombstone_ratio(0.2);
    assert_eq!(db.maintain(policy).unwrap().action, MaintenanceAction::None);
    db.put(1, &[1.0], "a").unwrap();
    assert_eq!(
        db.maintain(policy).unwrap().action,
        MaintenanceAction::Checkpoint
    );
    db.put(1, &[2.0], "b").unwrap();
    db.put(1, &[3.0], "c").unwrap();
    let status = db.maintenance_status(policy).unwrap();
    assert!(status.compact_recommended);
    let report = db.maintain(policy).unwrap();
    assert_eq!(report.action, MaintenanceAction::Compact);
    assert_eq!(report.removed_nodes, 2);
    assert_eq!(db.stats().tombstones, 0);
    for ratio in [f64::NAN, -0.1, 1.1] {
        assert!(db.maintain(policy.with_tombstone_ratio(ratio)).is_err());
    }
    assert_eq!(db.sequence(), 3);
    drop(db);
    assert_eq!(
        Database::open(temp.path())
            .unwrap()
            .get(1)
            .unwrap()
            .metadata,
        "c"
    );
}

#[test]
fn v1_snapshot_is_read_as_l2_and_migrated_on_checkpoint() {
    let temp = TempDir::new();
    let mut bytes = b"VECTORS1".to_vec();
    for value in [1u32, 1, 16, 200] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [42u64, 1, 1] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&7u64.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&3f32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"old");
    let c = crc(&bytes);
    bytes.extend_from_slice(&c.to_le_bytes());
    fs::write(temp.path().join("snapshot.bin"), bytes).unwrap();
    fs::write(temp.path().join("wal.bin"), []).unwrap();
    let mut db = Database::open(temp.path()).unwrap();
    assert_eq!(db.config().metric, Metric::SquaredL2);
    assert_eq!(db.get(7).unwrap().metadata, "old");
    assert!(!db.recovery_info().graph_cache_loaded);
    db.checkpoint().unwrap();
    drop(db);
    let bytes = fs::read(temp.path().join("snapshot.bin")).unwrap();
    assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 2);
    assert!(
        Database::open(temp.path())
            .unwrap()
            .recovery_info()
            .graph_cache_loaded
    );
}

#[test]
fn invalid_persisted_metric_and_cosine_zero_vector_fail_closed() {
    let temp = TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(1)).unwrap();
    db.put(1, &[0.0], "zero").unwrap();
    db.checkpoint().unwrap();
    drop(db);
    let original = fs::read(temp.path().join("snapshot.bin")).unwrap();
    for metric in [1u32, 999] {
        let mut bad = original.clone();
        bad[24..28].copy_from_slice(&metric.to_le_bytes());
        seal(&mut bad);
        fs::write(temp.path().join("snapshot.bin"), bad).unwrap();
        assert!(matches!(
            Database::open(temp.path()),
            Err(Error::Corrupt(_))
        ));
    }
}
#[test]
fn failed_cache_commit_keeps_authoritative_snapshot_and_wal_recoverable() {
    let temp = support::TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(1)).unwrap();
    db.put(1, &[1.0], "first").unwrap();
    db.put(2, &[2.0], "second").unwrap();
    // Fail after the new snapshot is renamed, before cache commit.
    std::fs::create_dir(temp.path().join("index.tmp")).unwrap();
    assert!(matches!(db.checkpoint(), Err(Error::Io(_))));
    assert!(matches!(db.put(3, &[3.0], ""), Err(Error::Poisoned)));
    drop(db);
    let mut db = Database::open(temp.path()).unwrap();
    assert_eq!(db.sequence(), 2);
    assert!(db.get(1).is_some() && db.get(2).is_some());
    assert!(!db.recovery_info().graph_cache_loaded);
    assert_eq!(db.recovery_info().skipped_frames, 2);
    assert_eq!(db.recovery_info().replayed_frames, 0);
    std::fs::remove_dir(temp.path().join("index.tmp")).unwrap();
    db.checkpoint().unwrap();
    drop(db);
    let db = Database::open(temp.path()).unwrap();
    assert!(db.recovery_info().graph_cache_loaded);
    assert_eq!(db.recovery_info().cached_nodes, 2);
}
