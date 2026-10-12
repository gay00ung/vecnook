#[path = "support/legacy_v03.rs"]
mod legacy;
mod support;
use std::{collections::BTreeMap, fs};
use support::TempDir;
use vecnook::{Collection, EmbeddingSpace, Error, Metric, doctor};
fn crc(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in bytes {
        c ^= u32::from(b);
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
fn inventory(path: &std::path::Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(path)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (
                e.file_name().to_str().unwrap().to_owned(),
                fs::read(e.path()).unwrap(),
            )
        })
        .collect()
}

#[test]
fn structured_authoritative_parser_mutations_never_panic_or_modify_source_files() {
    let tmp = TempDir::new();
    let path = tmp.path().join("legacy");
    fs::create_dir(&path).unwrap();
    let files = [
        ("snapshot.bin", legacy::SNAPSHOT),
        ("wal.bin", legacy::WAL),
        ("index.bin", legacy::INDEX),
        ("collection.bin", legacy::COLLECTION),
        ("LOCK", &[][..]),
    ];
    let mut seed = 0x20261006u64;
    for name in ["snapshot.bin", "wal.bin", "collection.bin"] {
        for iteration in 0..160 {
            for (n, b) in files {
                fs::write(path.join(n), b).unwrap();
            }
            let original = files.iter().find(|(n, _)| *n == name).unwrap().1;
            let mut data = original.to_vec();
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let position = (seed as usize) % data.len();
            match iteration % 4 {
                0 => data.truncate(position),
                1 => data[position] ^= 0x80,
                2 => {
                    data[position] = 0xff;
                    if name != "wal.bin" && data.len() >= 4 {
                        seal(&mut data);
                    }
                }
                _ => {
                    let width = (data.len() - position).min(8);
                    data[position..position + width].fill(0xff);
                    if name != "wal.bin" {
                        seal(&mut data);
                    }
                }
            }
            fs::write(path.join(name), data).unwrap();
            let before = inventory(&path);
            let result = std::panic::catch_unwind(|| doctor(&path));
            assert!(
                result.is_ok(),
                "seed={seed} file={name} iteration={iteration}"
            );
            assert_eq!(inventory(&path), before);
        }
    }
}

#[test]
fn a_future_snapshot_version_is_rejected_before_wal_tail_repair() {
    let tmp = TempDir::new();
    let path = tmp.path().join("legacy");
    fs::create_dir(&path).unwrap();
    let mut snapshot = legacy::SNAPSHOT.to_vec();
    snapshot[8..12].copy_from_slice(&99u32.to_le_bytes());
    seal(&mut snapshot);
    fs::write(path.join("snapshot.bin"), snapshot).unwrap();
    let mut wal = legacy::WAL.to_vec();
    wal.extend_from_slice(&[1, 2, 3]);
    fs::write(path.join("wal.bin"), wal).unwrap();
    fs::write(path.join("collection.bin"), legacy::COLLECTION).unwrap();
    fs::write(path.join("LOCK"), []).unwrap();
    let before = inventory(&path);
    let space = EmbeddingSpace::new("fixture-v03:stable-model", 2, Metric::Cosine);
    assert!(matches!(
        Collection::open(tmp.path(), "legacy", &space),
        Err(Error::Corrupt(_))
    ));
    assert_eq!(inventory(&path), before);
}
