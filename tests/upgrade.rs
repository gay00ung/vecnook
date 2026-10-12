#[path = "support/legacy_v03.rs"]
mod legacy;
mod support;
#[path = "support/legacy_vectors.rs"]
mod vectors;
use std::fs;
use support::TempDir;
use vecnook::{Collection, Document, DocumentFilter, EmbeddingSpace, Metric, SearchOptions};

#[test]
fn actual_v01_and_v02_vector_binary_files_open_and_migrate_on_checkpoint() {
    for (snapshot, wal) in [
        (vectors::V01_SNAPSHOT, vectors::V01_WAL),
        (vectors::V02_SNAPSHOT, vectors::V02_WAL),
    ] {
        let tmp = TempDir::new();
        fs::write(tmp.path().join("snapshot.bin"), snapshot).unwrap();
        fs::write(tmp.path().join("wal.bin"), wal).unwrap();
        let mut db = vecnook::Database::open(tmp.path()).unwrap();
        assert_eq!(db.config().metric, Metric::SquaredL2);
        assert_eq!(db.len(), 1);
        assert!(db.get(7).is_none());
        assert_eq!(db.get(u64::MAX).unwrap().vector, [1.0, 0.0]);
        assert_eq!(
            db.get(u64::MAX).unwrap().metadata,
            "updated old original 한글"
        );
        db.put(9, &[2.0, 0.0], "new write").unwrap();
        db.checkpoint().unwrap();
        let backup = tmp.path().join("backup");
        db.backup(&backup).unwrap();
        drop(db);
        for path in [tmp.path(), backup.as_path()] {
            let db = vecnook::Database::open(path).unwrap();
            assert_eq!(db.len(), 2);
            assert_eq!(
                db.search_exact(&[1.0, 0.0], 1).unwrap().neighbors[0].id,
                u64::MAX
            );
            db.check_invariants().unwrap();
        }
    }
}

#[test]
fn actual_v03_binary_fixture_upgrades_searches_mutates_checkpoints_backs_up_and_exports() {
    let tmp = TempDir::new();
    let path = tmp.path().join("legacy");
    fs::create_dir(&path).unwrap();
    for (name, data) in [
        ("snapshot.bin", legacy::SNAPSHOT),
        ("wal.bin", legacy::WAL),
        ("index.bin", legacy::INDEX),
        ("collection.bin", legacy::COLLECTION),
    ] {
        fs::write(path.join(name), data).unwrap();
    }
    let space = EmbeddingSpace::new("fixture-v03:stable-model", 2, Metric::Cosine);
    let mut c = Collection::open(tmp.path(), "legacy", &space).unwrap();
    assert_eq!(c.sequence(), 2);
    assert!(c.recovery_info().graph_cache_loaded);
    assert!(c.get(7).unwrap().is_none());
    let doc = c.get(u64::MAX).unwrap().unwrap();
    assert_eq!(doc.text, "updated legacy 원문\n\r\n");
    assert_eq!(doc.start_line, 8);
    assert_eq!(doc.end_line, 9);
    assert_eq!(c.vector(u64::MAX).unwrap(), [0.0, 1.0]);
    let found = c
        .search(
            &[0.0, 1.0],
            10,
            SearchOptions::default(),
            DocumentFilter::default().with_tags(&["rust"]),
        )
        .unwrap();
    assert_eq!(found.neighbors[0].document, doc);
    c.put(
        &Document::new(9, "new version write", "new.md"),
        &[1.0, 0.0],
    )
    .unwrap();
    c.checkpoint().unwrap();
    c.backup(tmp.path(), "backup").unwrap();
    let archive = tmp.path().join("upgraded.export");
    c.export(&archive).unwrap();
    drop(c);
    let imported = Collection::import(&archive, tmp.path(), "imported").unwrap();
    assert_eq!(imported.space(), &space);
    drop(imported);
    for name in ["legacy", "backup", "imported"] {
        let c = Collection::open(tmp.path(), name, &space).unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!(c.get(u64::MAX).unwrap(), Some(doc.clone()));
        assert_eq!(c.get(9).unwrap().unwrap().text, "new version write");
        c.check_invariants().unwrap();
    }
}
