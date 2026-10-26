use super::*;
use crate::storage_io::faults::{Guard, Mode};
use std::{
    sync::atomic::{AtomicU64, Ordering as AtomicOrdering},
    time::{SystemTime, UNIX_EPOCH},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "vecnook-gen-fault-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, AtomicOrdering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn store(path: &Path) -> Publisher {
    Publisher::create(
        path,
        Config::new(3),
        EmbeddingSpace::new("m", 3, Metric::SquaredL2),
    )
    .unwrap()
}
fn publish(publisher: &mut Publisher, seq: u64, label: &str) -> Snapshot {
    let mut builder = publisher.begin(seq, PublishOptions::default()).unwrap();
    builder
        .add(u64::MAX, &[1.0, -0.0, f32::from_bits(1)], label)
        .unwrap();
    builder.commit().unwrap()
}
#[test]
fn every_publication_io_boundary_preserves_an_entire_generation_and_requires_reopen() {
    for (point, mode) in [
        ("generation.segment.write", Mode::Partial(7)),
        ("generation.segment.sync", Mode::Fail),
        ("generation.manifest.write", Mode::Partial(13)),
        ("generation.manifest.sync", Mode::Fail),
        ("generation.directory.sync", Mode::Fail),
        ("generation.pointer.write", Mode::Partial(11)),
        ("generation.pointer.sync", Mode::Fail),
        ("generation.publish", Mode::Fail),
        ("generation.root.sync", Mode::Fail),
    ] {
        let temp = Temp::new();
        let path = temp.0.join("store");
        let mut publisher = store(&path);
        let old = publish(&mut publisher, 1, "old");
        let mut builder = publisher.begin(2, PublishOptions::default()).unwrap();
        builder
            .add(u64::MAX, &[3.0, -0.0, f32::from_bits(1)], "new")
            .unwrap();
        let guard = Guard::new(point, mode);
        assert!(matches!(builder.commit(), Err(Error::Io(_))), "{point}");
        drop(guard);
        assert!(matches!(
            publisher.begin(3, PublishOptions::default()),
            Err(Error::Poisoned)
        ));
        let opened = Snapshot::open(&path).unwrap();
        let expected = if point == "generation.root.sync" {
            "new"
        } else {
            "old"
        };
        assert_eq!(
            opened.get(u64::MAX).unwrap().unwrap().metadata,
            expected,
            "{point}"
        );
        assert_eq!(old.get(u64::MAX).unwrap().unwrap().metadata, "old");
        drop(publisher);
        let mut reopened = Publisher::open(&path).unwrap();
        let next = publish(&mut reopened, 3, "recovered");
        assert_eq!(next.get(u64::MAX).unwrap().unwrap().metadata, "recovered");
    }
}
#[test]
fn fragmented_and_interrupted_publication_writes_complete_without_partial_rows() {
    for point in [
        "generation.segment.write",
        "generation.manifest.write",
        "generation.pointer.write",
    ] {
        for mode in [Mode::Short, Mode::Interrupted] {
            let temp = Temp::new();
            let mut publisher = store(&temp.0.join("store"));
            let mut builder = publisher.begin(1, PublishOptions::default()).unwrap();
            builder.add(7, &[1., -0., 3.], "original").unwrap();
            let guard = Guard::new(point, mode);
            let snapshot = builder.commit().unwrap();
            drop(guard);
            assert_eq!(
                snapshot.get(7).unwrap().unwrap().vector[1].to_bits(),
                (-0.0f32).to_bits()
            );
            assert_eq!(snapshot.sequence(), 1);
        }
    }
}
fn rewrite_manifest(path: &Path, generation: u64, mutate: impl FnOnce(&mut Vec<u8>)) {
    let manifest = path.join(generation_name(generation)).join("MANIFEST");
    let mut bytes = fs::read(&manifest).unwrap();
    mutate(&mut bytes);
    let checksum = storage::crc32(&bytes[..bytes.len() - 4]);
    let end = bytes.len();
    bytes[end - 4..].copy_from_slice(&checksum.to_le_bytes());
    fs::write(manifest, bytes).unwrap();
    let mut pointer = fs::read(path.join("CURRENT")).unwrap();
    pointer[16..20].copy_from_slice(&checksum.to_le_bytes());
    let crc = storage::crc32(&pointer[..20]);
    pointer[20..].copy_from_slice(&crc.to_le_bytes());
    fs::write(path.join("CURRENT"), pointer).unwrap();
}
#[test]
fn valid_checksums_do_not_hide_overlapping_offsets_invalid_ids_or_nonfinite_vectors() {
    // The fixture identity "m" gives 62 header bytes before segment descriptors;
    // descriptors are 12 bytes and row entries are 20 bytes.
    for attack in ["overlap", "duplicate", "count", "identity", "nan", "utf8"] {
        let temp = Temp::new();
        let path = temp.0.join("store");
        let mut publisher = store(&path);
        let mut builder = publisher.begin(1, PublishOptions::default()).unwrap();
        builder.add(1, &[1., 2., 3.], "a").unwrap();
        builder.add(2, &[4., 5., 6.], "b").unwrap();
        let snapshot = builder.commit().unwrap();
        let generation = snapshot.generation();
        let first_entry = 62 + 12;
        match attack {
            "overlap" => rewrite_manifest(&path, generation, |bytes| {
                bytes[first_entry + 20 + 12..first_entry + 20 + 16]
                    .copy_from_slice(&0u32.to_le_bytes());
            }),
            "duplicate" => rewrite_manifest(&path, generation, |bytes| {
                bytes[first_entry + 20..first_entry + 28].copy_from_slice(&1u64.to_le_bytes());
            }),
            "count" => rewrite_manifest(&path, generation, |bytes| {
                bytes[54..62].copy_from_slice(&u64::MAX.to_le_bytes());
            }),
            "identity" => rewrite_manifest(&path, generation, |bytes| {
                bytes[49] = b'n';
            }),
            "nan" | "utf8" => {
                let segment = path.join(generation_name(generation)).join(segment_name(0));
                let mut bytes = fs::read(&segment).unwrap();
                if attack == "nan" {
                    bytes[12..16].copy_from_slice(&f32::NAN.to_le_bytes());
                } else {
                    bytes[24] = 255;
                }
                let end = bytes.len();
                let crc = storage::crc32(&bytes[..end - 4]);
                bytes[end - 4..].copy_from_slice(&crc.to_le_bytes());
                fs::write(segment, bytes).unwrap();
                rewrite_manifest(&path, generation, |bytes| {
                    bytes[70..74].copy_from_slice(&crc.to_le_bytes());
                });
            }
            _ => unreachable!(),
        }
        assert!(
            matches!(Snapshot::open(&path), Err(Error::Corrupt(_))),
            "{attack}"
        );
    }
}
