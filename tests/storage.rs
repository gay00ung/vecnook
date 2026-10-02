mod support;
use std::{
    fs::{self, OpenOptions},
    io::Write,
};
use support::TempDir;
use vecnook::{Config, Database, Error};

#[test]
fn wal_replays_insert_update_delete_and_utf8_metadata() {
    let temp = TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(2)).unwrap();
    db.put(1, &[1.0, 2.0], "old").unwrap();
    db.put(2, &[3.0, 4.0], "삭제").unwrap();
    db.put(1, &[5.0, 6.0], "한글 🐈\nnew").unwrap();
    db.delete(2).unwrap();
    assert!(!db.delete(2).unwrap());
    assert_eq!(db.sequence(), 4);
    let before = db.search_hnsw(&[0.0, 0.0], 10, 128).unwrap();
    drop(db);
    let db = Database::open(temp.path()).unwrap();
    assert_eq!(db.recovery_info().replayed_frames, 4);
    assert_eq!(db.get(1).unwrap().vector, [5.0, 6.0]);
    assert_eq!(db.get(1).unwrap().metadata, "한글 🐈\nnew");
    assert!(db.get(2).is_none());
    let after = db.search_hnsw(&[0.0, 0.0], 10, 128).unwrap();
    assert_eq!(before.neighbors, after.neighbors);
    assert_eq!(before.distance_computations, after.distance_computations);
    db.check_invariants().unwrap();
}

#[test]
fn checkpoint_preserves_tombstones_and_next_wal_sequence() {
    let temp = TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(2)).unwrap();
    db.put(1, &[0.0, 0.0], "old").unwrap();
    db.put(1, &[1.0, 2.0], "new").unwrap();
    db.checkpoint().unwrap();
    assert_eq!(fs::metadata(temp.path().join("wal.bin")).unwrap().len(), 0);
    db.put(2, &[3.0, 4.0], "after checkpoint").unwrap();
    drop(db);
    let db = Database::open(temp.path()).unwrap();
    assert_eq!(db.sequence(), 3);
    assert_eq!(db.recovery_info().replayed_frames, 1);
    assert_eq!(db.stats().tombstones, 1);
    assert_eq!(db.get(1).unwrap().metadata, "new");
    assert!(db.get(2).is_some());
}

#[test]
fn old_wal_after_snapshot_commit_is_validated_but_not_reapplied() {
    let temp = TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(2)).unwrap();
    db.put(1, &[1.0, 2.0], "once").unwrap();
    let old = fs::read(temp.path().join("wal.bin")).unwrap();
    db.checkpoint().unwrap();
    db.put(2, &[3.0, 4.0], "twice").unwrap();
    let new = fs::read(temp.path().join("wal.bin")).unwrap();
    drop(db);
    fs::write(temp.path().join("wal.bin"), [old, new].concat()).unwrap();
    let db = Database::open(temp.path()).unwrap();
    assert_eq!(db.recovery_info().skipped_frames, 1);
    assert_eq!(db.recovery_info().replayed_frames, 1);
    assert_eq!(db.stats().physical_nodes, 2);
    assert_eq!(db.stats().active_records, 2);
}

#[test]
fn compact_and_reopen_preserve_only_current_active_records() {
    let temp = TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(2)).unwrap();
    for id in 0..40 {
        db.put(id, &[id as f32, 1.0], "old").unwrap();
    }
    for id in 0..20 {
        db.delete(id).unwrap();
    }
    for id in 20..30 {
        db.put(id, &[id as f32, 2.0], "new").unwrap();
    }
    assert_eq!(db.compact().unwrap(), 30);
    let expected = db.search_exact(&[25.1, 2.0], 10).unwrap().neighbors;
    drop(db);
    let db = Database::open(temp.path()).unwrap();
    assert_eq!(db.stats().physical_nodes, 20);
    assert_eq!(db.stats().tombstones, 0);
    assert_eq!(
        db.search_hnsw(&[25.1, 2.0], 10, 128).unwrap().neighbors,
        expected
    );
}

#[test]
fn partial_tail_is_truncated_at_every_byte_boundary() {
    let source = TempDir::new();
    let mut db = Database::create(source.path(), Config::new(2)).unwrap();
    db.put(1, &[1.0, 2.0], "committed").unwrap();
    let first_len = fs::metadata(source.path().join("wal.bin")).unwrap().len() as usize;
    db.put(2, &[3.0, 4.0], "unfinished").unwrap();
    drop(db);
    let snapshot = fs::read(source.path().join("snapshot.bin")).unwrap();
    let wal = fs::read(source.path().join("wal.bin")).unwrap();
    // Covers incomplete prefix, body, UTF-8 payload and each CRC boundary.
    for cut in first_len + 1..wal.len() {
        let target = TempDir::new();
        fs::write(target.path().join("snapshot.bin"), &snapshot).unwrap();
        fs::write(target.path().join("wal.bin"), &wal[..cut]).unwrap();
        let mut recovered = Database::open(target.path()).unwrap();
        assert_eq!(
            recovered.recovery_info().truncated_tail_bytes,
            (cut - first_len) as u64
        );
        assert!(recovered.get(1).is_some());
        assert!(recovered.get(2).is_none());
        assert_eq!(
            fs::metadata(target.path().join("wal.bin")).unwrap().len(),
            first_len as u64
        );
        recovered.put(3, &[5.0, 6.0], "after repair").unwrap();
        drop(recovered);
        assert!(Database::open(target.path()).unwrap().get(3).is_some());
    }
}

#[test]
fn complete_wal_checksum_error_fails_without_truncating_data() {
    let temp = TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(1)).unwrap();
    db.put(1, &[1.0], "saved").unwrap();
    drop(db);
    let path = temp.path().join("wal.bin");
    let mut bytes = fs::read(&path).unwrap();
    *bytes.last_mut().unwrap() ^= 0x80;
    fs::write(&path, &bytes).unwrap();
    assert!(matches!(
        Database::open(temp.path()),
        Err(Error::Corrupt(_))
    ));
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn snapshot_checksum_error_is_not_silently_rebuilt() {
    let temp = TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(2)).unwrap();
    db.put(1, &[1.0, 2.0], "saved").unwrap();
    db.checkpoint().unwrap();
    drop(db);
    let path = temp.path().join("snapshot.bin");
    let mut bytes = fs::read(&path).unwrap();
    bytes[20] ^= 1;
    fs::write(&path, &bytes).unwrap();
    assert!(matches!(
        Database::open(temp.path()),
        Err(Error::Corrupt(_))
    ));
    assert_eq!(fs::read(path).unwrap(), bytes);
}

fn checksum(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb88320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}
fn reseal_snapshot(bytes: &mut [u8]) {
    let end = bytes.len() - 4;
    let crc = checksum(&bytes[..end]);
    bytes[end..].copy_from_slice(&crc.to_le_bytes());
}

#[test]
fn checksummed_snapshot_fields_are_validated_before_allocation() {
    let temp = TempDir::new();
    let db = Database::create(temp.path(), Config::new(2)).unwrap();
    drop(db);
    let original = fs::read(temp.path().join("snapshot.bin")).unwrap();
    for (offset, field) in [(8, 999u32), (12, u32::MAX), (16, 1u32), (20, u32::MAX)] {
        let mut bytes = original.clone();
        bytes[offset..offset + 4].copy_from_slice(&field.to_le_bytes());
        reseal_snapshot(&mut bytes);
        fs::write(temp.path().join("snapshot.bin"), bytes).unwrap();
        assert!(matches!(
            Database::open(temp.path()),
            Err(Error::Corrupt(_))
        ));
    }
    let mut bytes = original;
    bytes[44..52].copy_from_slice(&u64::MAX.to_le_bytes());
    reseal_snapshot(&mut bytes);
    fs::write(temp.path().join("snapshot.bin"), bytes).unwrap();
    assert!(matches!(
        Database::open(temp.path()),
        Err(Error::Corrupt(_))
    ));
}

#[test]
fn wal_sequence_gap_is_rejected_even_with_valid_checksum() {
    let temp = TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(1)).unwrap();
    db.put(1, &[1.0], "").unwrap();
    drop(db);
    let path = temp.path().join("wal.bin");
    let mut bytes = fs::read(&path).unwrap();
    let length = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    bytes[9..17].copy_from_slice(&3u64.to_le_bytes());
    let crc = checksum(&bytes[8..8 + length]);
    bytes[8 + length..].copy_from_slice(&crc.to_le_bytes());
    fs::write(&path, &bytes).unwrap();
    assert!(matches!(
        Database::open(temp.path()),
        Err(Error::Corrupt(_))
    ));
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn impossible_wal_length_is_rejected() {
    let temp = TempDir::new();
    let db = Database::create(temp.path(), Config::new(1)).unwrap();
    drop(db);
    let length = u32::MAX.to_le_bytes();
    let header = [length.to_vec(), checksum(&length).to_le_bytes().to_vec()].concat();
    fs::write(temp.path().join("wal.bin"), header).unwrap();
    assert!(matches!(
        Database::open(temp.path()),
        Err(Error::Corrupt(_))
    ));
}

#[test]
fn corrupted_but_plausible_wal_length_is_not_mistaken_for_a_partial_tail() {
    let temp = TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(1)).unwrap();
    db.put(1, &[1.0], "acknowledged").unwrap();
    drop(db);
    let path = temp.path().join("wal.bin");
    let mut bytes = fs::read(&path).unwrap();
    let length = u32::from_le_bytes(bytes[..4].try_into().unwrap());
    bytes[..4].copy_from_slice(&(length + 1).to_le_bytes());
    fs::write(&path, &bytes).unwrap();
    assert!(matches!(
        Database::open(temp.path()),
        Err(Error::Corrupt(_))
    ));
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn checksummed_snapshot_rejects_bad_flag_float_length_and_utf8() {
    let temp = TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(1)).unwrap();
    db.put(1, &[1.0], "ok").unwrap();
    db.checkpoint().unwrap();
    drop(db);
    let original = fs::read(temp.path().join("snapshot.bin")).unwrap();
    let mutations = [
        (60, vec![2]),
        (61, f32::NAN.to_le_bytes().to_vec()),
        (65, 16_385u32.to_le_bytes().to_vec()),
        (69, vec![255]),
    ];
    for (offset, replacement) in mutations {
        let mut bytes = original.clone();
        bytes[offset..offset + replacement.len()].copy_from_slice(&replacement);
        reseal_snapshot(&mut bytes);
        fs::write(temp.path().join("snapshot.bin"), bytes).unwrap();
        assert!(matches!(
            Database::open(temp.path()),
            Err(Error::Corrupt(_))
        ));
    }
}

#[test]
fn checksummed_unknown_wal_operation_is_rejected() {
    let temp = TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(1)).unwrap();
    db.put(1, &[1.0], "").unwrap();
    drop(db);
    let path = temp.path().join("wal.bin");
    let mut bytes = fs::read(&path).unwrap();
    let length = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    bytes[8] = 255;
    let crc = checksum(&bytes[8..8 + length]);
    bytes[8 + length..].copy_from_slice(&crc.to_le_bytes());
    fs::write(path, bytes).unwrap();
    assert!(matches!(
        Database::open(temp.path()),
        Err(Error::Corrupt(_))
    ));
}

#[test]
fn competing_handles_are_locked_and_drop_releases_the_lock() {
    let temp = TempDir::new();
    let db = Database::create(temp.path(), Config::new(1)).unwrap();
    assert!(matches!(Database::open(temp.path()), Err(Error::Locked)));
    assert!(matches!(
        Database::create(temp.path(), Config::new(1)),
        Err(Error::Locked)
    ));
    drop(db);
    let reopened = Database::open(temp.path()).unwrap();
    drop(reopened);
    assert!(matches!(
        Database::create(temp.path(), Config::new(1)),
        Err(Error::AlreadyExists)
    ));
}

#[test]
fn invalid_mutation_never_appends_wal_or_advances_sequence() {
    let temp = TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(2)).unwrap();
    db.put(1, &[1.0, 2.0], "original").unwrap();
    let length = fs::metadata(temp.path().join("wal.bin")).unwrap().len();
    assert!(db.put(1, &[1.0], "bad").is_err());
    assert!(db.put(1, &[f32::NAN, 2.0], "bad").is_err());
    assert!(db.put(1, &[1.0, 2.0], &"x".repeat(16 * 1024 + 1)).is_err());
    assert_eq!(db.sequence(), 1);
    assert_eq!(
        fs::metadata(temp.path().join("wal.bin")).unwrap().len(),
        length
    );
    assert_eq!(db.get(1).unwrap().metadata, "original");
}

#[test]
fn missing_wal_is_not_recreated_over_an_older_snapshot() {
    let temp = TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(1)).unwrap();
    db.put(1, &[1.0], "acknowledged").unwrap();
    drop(db);
    fs::remove_file(temp.path().join("wal.bin")).unwrap();
    assert!(matches!(Database::open(temp.path()), Err(Error::Io(_))));
    assert!(!temp.path().join("wal.bin").exists());
}

#[test]
fn temporary_snapshot_is_ignored_when_committed_snapshot_is_valid() {
    let temp = TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(1)).unwrap();
    db.put(1, &[1.0], "saved").unwrap();
    drop(db);
    fs::write(
        temp.path().join("snapshot.tmp"),
        b"partial uncommitted data",
    )
    .unwrap();
    let mut db = Database::open(temp.path()).unwrap();
    assert!(db.get(1).is_some());
    db.checkpoint().unwrap();
    assert!(!temp.path().join("snapshot.tmp").exists());
}

#[test]
fn trailing_partial_prefix_is_repaired_and_reported() {
    let temp = TempDir::new();
    let db = Database::create(temp.path(), Config::new(1)).unwrap();
    drop(db);
    let mut file = OpenOptions::new()
        .append(true)
        .open(temp.path().join("wal.bin"))
        .unwrap();
    file.write_all(&[1, 2, 3]).unwrap();
    drop(file);
    let db = Database::open(temp.path()).unwrap();
    assert_eq!(db.recovery_info().truncated_tail_bytes, 3);
    assert_eq!(db.stats().active_records, 0);
}
