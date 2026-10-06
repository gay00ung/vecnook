mod support;
use std::fs;
use support::TempDir;
use vecnook::{
    Collection, Config, Database, Document, DocumentFilter, DocumentMutation, EmbeddingSpace,
    Error, Metric, SearchOptions,
};

fn space() -> EmbeddingSpace {
    EmbeddingSpace::new("model-sha256/prompt-v1", 2, Metric::Cosine)
}

#[test]
fn large_tag_intersections_use_graph_paths_and_exact_results_match_an_independent_oracle() {
    let temp = TempDir::new();
    let mut collection =
        Collection::create(temp.path(), "large", Config::new(1), "l2-fixture-v1").unwrap();
    let documents: Vec<_> = (0..600)
        .map(|id| {
            doc(
                id,
                if id % 3 == 0 { "a.md" } else { "b.md" },
                &["all", if id % 2 == 0 { "even" } else { "odd" }],
            )
        })
        .collect();
    let vectors: Vec<_> = (0..600).map(|id| vec![id as f32]).collect();
    let operations: Vec<_> = documents
        .iter()
        .zip(&vectors)
        .map(|(document, vector)| DocumentMutation::Put { document, vector })
        .collect();
    collection.write_batch(&operations).unwrap();
    let filter = DocumentFilter::default()
        .with_optional_source(None)
        .with_tags(&["all", "even"]);
    let query = [123.4f32];
    let exact = collection
        .search(
            &query,
            10,
            SearchOptions::default().with_strategy(vecnook::SearchStrategy::Exact),
            filter,
        )
        .unwrap();
    let mut oracle: Vec<_> = (0..600u64)
        .filter(|id| id % 2 == 0)
        .map(|id| ((id as f64 - query[0] as f64).powi(2), id))
        .collect();
    oracle.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    assert_eq!(
        exact
            .neighbors
            .iter()
            .map(|n| n.document.id)
            .collect::<Vec<_>>(),
        oracle[..10].iter().map(|x| x.1).collect::<Vec<_>>()
    );
    let graph = collection
        .search(&query, 10, SearchOptions::default(), filter)
        .unwrap();
    assert_eq!(graph.search.eligible_count, 300);
    assert_eq!(graph.search.mode, vecnook::SearchMode::Hnsw);
    assert!(graph.search.complete);
    assert!(
        graph
            .neighbors
            .iter()
            .all(|n| n.document.tags.contains(&"even".to_string()))
    );
    let intersected = collection
        .search(
            &query,
            10,
            SearchOptions::default(),
            DocumentFilter::default()
                .with_optional_source(Some("a.md"))
                .with_tags(&["even"]),
        )
        .unwrap();
    assert_eq!(intersected.search.eligible_count, 100);
    assert!(intersected.neighbors.iter().all(|n| n.document.id % 6 == 0));
}
fn create(root: &std::path::Path, name: &str) -> Collection {
    Collection::create(
        root,
        name,
        Config::new(2).with_metric(Metric::Cosine),
        &space().model,
    )
    .unwrap()
}
fn doc(id: u64, source: &str, tags: &[&str]) -> Document {
    let mut doc = Document::new(id, "본문\n\tquoted \"text\" 🚀", source);
    doc.start_line = 3;
    doc.end_line = 7;
    doc.tags = tags.iter().map(|s| s.to_string()).collect();
    doc
}

#[test]
fn document_codec_roundtrips_unicode_and_rejects_every_truncation_and_bad_length() {
    let doc = doc(u64::MAX, "한글 path.md", &["rust", "태그"]);
    let encoded = doc.to_payload().unwrap();
    assert_eq!(Document::from_payload(&encoded).unwrap(), doc);
    for size in 0..encoded.len() {
        assert!(
            Document::from_payload(&encoded[..size]).is_err(),
            "size={size}"
        );
    }
    assert!(Document::from_payload(&(encoded.clone() + "00")).is_err());
    assert!(Document::from_payload(&encoded.replace("VDOC1:", "VDOC2:")).is_err());
    let mut bytes = encoded.into_bytes();
    bytes[6] = b'z';
    assert!(Document::from_payload(std::str::from_utf8(&bytes).unwrap()).is_err());
    let invalid = "VDOC1:".to_string() + &"00".repeat(16) + "ffffffff";
    assert!(Document::from_payload(&invalid).is_err());
}

#[test]
fn collection_model_namespace_tags_source_backup_and_reopen_are_enforced() {
    let temp = TempDir::new();
    let mut first = create(temp.path(), "first");
    let mut second = create(temp.path(), "second");
    let a = doc(1, "a.md", &["rust", "storage"]);
    let b = doc(2, "b.md", &["rust"]);
    first.put(&a, &[1.0, 0.0]).unwrap();
    first.put(&b, &[0.0, 1.0]).unwrap();
    second
        .put(&doc(1, "private.md", &["other"]), &[1.0, 0.0])
        .unwrap();
    assert_eq!(first.len(), 2);
    assert_eq!(second.len(), 1);
    assert_eq!(first.vector(1), Some([1.0, 0.0].as_slice()));
    assert_eq!(Collection::describe(temp.path(), "first").unwrap(), space());
    let report = first
        .search(
            &[1.0, 0.0],
            10,
            SearchOptions::default(),
            DocumentFilter::default()
                .with_optional_source(Some("a.md"))
                .with_tags(&["rust", "storage", "rust"]),
        )
        .unwrap();
    assert_eq!(report.neighbors[0].document, a);
    assert_eq!(report.search.eligible_count, 1);
    assert_eq!(report.search.filter_evaluations, 0);
    assert!(
        first
            .search(
                &[1.0, 0.0],
                10,
                SearchOptions::default(),
                DocumentFilter::default()
                    .with_optional_source(None)
                    .with_tags(&["missing"])
            )
            .unwrap()
            .neighbors
            .is_empty()
    );
    assert!(matches!(
        Collection::open(temp.path(), "first", &space()),
        Err(Error::Locked)
    ));
    assert!(
        Collection::open(
            temp.path(),
            "first",
            &EmbeddingSpace::new("different", 2, Metric::Cosine)
        )
        .is_err()
    );
    first.backup(temp.path(), "copy").unwrap();
    assert!(matches!(
        first.backup(temp.path(), "copy"),
        Err(Error::AlreadyExists)
    ));
    drop(first);
    for name in ["first", "copy"] {
        let collection = Collection::open(temp.path(), name, &space()).unwrap();
        assert_eq!(collection.get(1).unwrap(), Some(a.clone()));
        assert_eq!(collection.get(2).unwrap(), Some(b.clone()));
        collection.check_invariants().unwrap();
    }
    for wrong in [
        EmbeddingSpace::new("different", 2, Metric::Cosine),
        EmbeddingSpace::new(space().model, 3, Metric::Cosine),
        EmbeddingSpace::new(space().model, 2, Metric::SquaredL2),
    ] {
        assert!(matches!(
            Collection::open(temp.path(), "first", &wrong),
            Err(Error::EmbeddingMismatch)
        ));
    }
}

#[test]
fn ordered_batch_updates_postings_atomically_and_compaction_preserves_them() {
    let temp = TempDir::new();
    let mut collection = create(temp.path(), "notes");
    let old = doc(1, "old.md", &["old"]);
    let new = doc(1, "new.md", &["new"]);
    collection.put(&old, &[1.0, 0.0]).unwrap();
    let before = fs::read(temp.path().join("notes/wal.bin")).unwrap();
    let mut invalid = doc(9, "bad.md", &["a", "a"]);
    assert!(
        collection
            .write_batch(&[
                DocumentMutation::Delete { id: 1 },
                DocumentMutation::Put {
                    document: &invalid,
                    vector: &[1.0, 0.0]
                }
            ])
            .is_err()
    );
    assert_eq!(fs::read(temp.path().join("notes/wal.bin")).unwrap(), before);
    assert_eq!(collection.get(1).unwrap(), Some(old));
    invalid.tags = vec![];
    assert!(collection.put(&invalid, &[0.0, 0.0]).is_err());
    let report = collection
        .write_batch(&[
            DocumentMutation::Put {
                document: &new,
                vector: &[0.0, 1.0],
            },
            DocumentMutation::Delete { id: 1 },
            DocumentMutation::Put {
                document: &new,
                vector: &[1.0, 0.0],
            },
        ])
        .unwrap();
    assert_eq!((report.updated, report.deleted, report.inserted), (1, 1, 1));
    for phase in 0..3 {
        assert_eq!(collection.get(1).unwrap(), Some(new.clone()));
        assert!(
            collection
                .search(
                    &[1.0, 0.0],
                    10,
                    SearchOptions::default(),
                    DocumentFilter::default()
                        .with_optional_source(None)
                        .with_tags(&["old"])
                )
                .unwrap()
                .neighbors
                .is_empty()
        );
        assert_eq!(
            collection
                .search(
                    &[1.0, 0.0],
                    10,
                    SearchOptions::default(),
                    DocumentFilter::default()
                        .with_optional_source(Some("new.md"))
                        .with_tags(&["new"])
                )
                .unwrap()
                .neighbors
                .len(),
            1
        );
        collection.check_invariants().unwrap();
        if phase == 0 {
            drop(collection);
            collection = Collection::open(temp.path(), "notes", &space()).unwrap();
        }
        if phase == 1 {
            collection.compact().unwrap();
            collection.checkpoint().unwrap();
            drop(collection);
            collection = Collection::open(temp.path(), "notes", &space()).unwrap();
        }
    }
    assert!(collection.delete(1).unwrap());
    assert!(!collection.delete(1).unwrap());
    assert!(collection.is_empty());
    collection.check_invariants().unwrap();
}

#[test]
fn payload_and_header_corruption_fail_closed_and_names_cannot_traverse() {
    let temp = TempDir::new();
    for name in [
        "",
        "../escape",
        "a/b",
        "a\\b",
        ".",
        "CON",
        "lpt1",
        "has space",
    ] {
        assert!(Collection::create(temp.path(), name, Config::new(2), "model").is_err());
    }
    let collection = create(temp.path(), "notes");
    drop(collection);
    assert!(matches!(
        Collection::create(temp.path(), "notes", Config::new(2), "model"),
        Err(Error::AlreadyExists)
    ));
    let header = temp.path().join("notes/collection.bin");
    let bytes = fs::read(&header).unwrap();
    for size in 0..bytes.len() {
        fs::write(&header, &bytes[..size]).unwrap();
        assert!(Collection::describe(temp.path(), "notes").is_err());
    }
    let mut damaged = bytes.clone();
    damaged[12] ^= 1;
    fs::write(&header, damaged).unwrap();
    assert!(matches!(
        Collection::open(temp.path(), "notes", &space()),
        Err(Error::Corrupt(_))
    ));
    fs::write(&header, bytes).unwrap();
    let mut db = Database::open(temp.path().join("notes")).unwrap();
    db.put(1, &[1.0, 0.0], "not a document payload").unwrap();
    drop(db);
    assert!(matches!(
        Collection::open(temp.path(), "notes", &space()),
        Err(Error::Corrupt(_))
    ));
}

#[test]
fn collection_symlink_and_document_size_limits_are_rejected() {
    let temp = TempDir::new();
    let outside = temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, temp.path().join("link")).unwrap();
        assert!(Collection::create(temp.path(), "link", Config::new(2), "model").is_err());
    }
    let mut value = doc(1, "a.md", &[]);
    value.text = "a".repeat(6001);
    assert!(value.to_payload().is_err());
    value.text.clear();
    value.start_line = 0;
    assert!(value.to_payload().is_err());
    value.start_line = 8;
    value.end_line = 7;
    assert!(value.to_payload().is_err());
    value.start_line = 1;
    value.source = "x".repeat(1025);
    assert!(value.to_payload().is_err());
    value.source = "a.md".into();
    value.text = "a".repeat(6000);
    value.tags = (0..32)
        .map(|n| format!("{n:03}{}", "x".repeat(125)))
        .collect();
    assert!(value.to_payload().is_err());
}
