mod support;
use support::TempDir;
use vecnook::{
    Collection, Config, Document, DocumentFilter, EmbeddingSpace, ErrorKind, FilterExpr,
    HybridSearchOptions, KeywordAnalyzer, MaintenancePolicy, app,
};

#[test]
fn non_hangul_keyword_scores_do_not_depend_on_korean_expansion() {
    let t = TempDir::new();
    let mut c = Collection::create(t.path(), "notes", Config::new(2), "fixture").unwrap();
    for (id, text, tag) in [
        (
            1,
            "backup restore ERR_IO_27 résumé café 백업을안전하게복원합니다",
            "keep",
        ),
        (
            2,
            "backup restore restore ERR_IO_27 résumé café 백업",
            "keep",
        ),
        (3, "backup restore storage notes", "hidden"),
        (4, "--- ___ !", "keep"),
    ] {
        let mut doc = Document::new(id, text, "mixed.md");
        doc.tags = vec![tag.into()];
        c.put(&doc, &[id as f32, 0.0]).unwrap();
    }
    let assert_scores = |c: &mut Collection| {
        for tags in [&[][..], &["keep"][..]] {
            for query in [
                "backup restore",
                "ERR_IO_27",
                "RÉSUMÉ café",
                "storage",
                "---",
            ] {
                let filter = DocumentFilter::default().with_tags(tags);
                c.set_keyword_analyzer(KeywordAnalyzer::Words).unwrap();
                let words = c.search_keywords(query, 10, filter).unwrap();
                c.set_keyword_analyzer(KeywordAnalyzer::KoreanNgram)
                    .unwrap();
                let korean = c.search_keywords(query, 10, filter).unwrap();
                assert_eq!(words.neighbors, korean.neighbors, "{query}");
                assert_eq!(words.matching_count, korean.matching_count);
                assert_eq!(words.postings_visited, korean.postings_visited);
            }
        }
        c.check_invariants().unwrap();
    };
    assert_scores(&mut c);
    // Mutate while the expanded analyzer owns the derived index, then rebuild
    // both policies as an independent check of replacement/deletion accounting.
    c.put(
        &Document::new(2, "restore café 백업은안전합니다", "other.md"),
        &[2.0, 0.0],
    )
    .unwrap();
    c.delete(3).unwrap();
    c.put(
        &Document::new(5, "backup ERR_IO_27 백업을", "new.md"),
        &[5.0, 0.0],
    )
    .unwrap();
    c.check_invariants().unwrap();
    assert_scores(&mut c);
    c.checkpoint().unwrap();
    let space = c.space().clone();
    drop(c);
    let mut c = Collection::open(t.path(), "notes", &space).unwrap();
    assert_scores(&mut c);
    assert!(
        c.search_keywords("백업", 10, DocumentFilter::default())
            .unwrap()
            .neighbors
            .iter()
            .any(|n| n.document.id == 5)
    );
}

#[test]
fn korean_bm25_counts_original_positions_and_deduplicates_grams_within_a_word() {
    let t = TempDir::new();
    let mut c = Collection::create(t.path(), "notes", Config::new(1), "fixture").unwrap();
    c.set_keyword_analyzer(KeywordAnalyzer::KoreanNgram)
        .unwrap();
    for (id, text) in [(1, "백업을 백업을"), (2, "백업"), (3, "else"), (4, "")] {
        c.put(&Document::new(id, text, "fixture.md"), &[id as f32])
            .unwrap();
    }
    let found = c
        .search_keywords("백업 백업", 10, DocumentFilter::default())
        .unwrap();
    assert_eq!(
        found
            .neighbors
            .iter()
            .map(|n| n.document.id)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    // Four original word positions, including two occurrences in document 1.
    // The whole two-syllable word in document 2 is also a gram, counted once.
    // N=4, df=2, average original length=1; repeated query terms count once.
    let expected = 2f64.ln() * 2.0 * 2.2 / (2.0 + 1.2 * (0.25 + 0.75 * 2.0));
    assert!((found.neighbors[0].score - expected).abs() < 1e-12);
    assert!((found.neighbors[1].score - 2f64.ln()).abs() < 1e-12);
    c.check_invariants().unwrap();
}

#[test]
fn selected_analyzer_survives_mutations_and_context_cannot_bypass_eligibility() {
    let t = TempDir::new();
    let mut c = Collection::create(t.path(), "notes", Config::new(2), "fixture").unwrap();
    for (id, text, tag) in [
        (1, "백업을 복원합니다 ERR_IO_27", "keep"),
        (2, "secret", "hidden"),
        (3, "백업은 안전합니다", "keep"),
    ] {
        let mut doc = Document::new(id, text, "same.md");
        doc.start_line = id as u32;
        doc.end_line = id as u32;
        doc.tags = vec![tag.into()];
        c.put(&doc, &[id as f32, 0.0]).unwrap();
    }
    assert!(
        c.search_keywords("백업", 10, DocumentFilter::default())
            .unwrap()
            .neighbors
            .is_empty()
    );
    c.set_keyword_analyzer(KeywordAnalyzer::KoreanNgram)
        .unwrap();
    let found = c
        .search_keywords("백업", 10, DocumentFilter::default())
        .unwrap();
    assert_eq!(
        found
            .neighbors
            .iter()
            .map(|n| n.document.id)
            .collect::<std::collections::BTreeSet<_>>(),
        [1, 3].into()
    );
    assert_eq!(
        c.search_keywords("ERR_IO_27", 1, DocumentFilter::default())
            .unwrap()
            .neighbors[0]
            .document
            .id,
        1
    );
    let filter = DocumentFilter::default().with_tags(&["keep"]);
    assert_eq!(
        c.context(1, 3, filter)
            .unwrap()
            .iter()
            .map(|d| d.id)
            .collect::<Vec<_>>(),
        [1, 3]
    );
    assert!(c.context(2, 3, filter).unwrap().is_empty());
    assert!(c.context(1, 4, filter).is_err());
    c.delete(1).unwrap();
    assert_eq!(
        c.search_keywords("백업", 10, filter).unwrap().neighbors[0]
            .document
            .id,
        3
    );
    c.compact().unwrap();
    c.check_invariants().unwrap();
    c.checkpoint().unwrap();
    drop(c);
    let mut c = Collection::open(
        t.path(),
        "notes",
        &EmbeddingSpace::new("fixture", 2, vecnook::Metric::SquaredL2),
    )
    .unwrap();
    assert_eq!(c.keyword_analyzer(), KeywordAnalyzer::Words);
    c.set_keyword_analyzer(KeywordAnalyzer::KoreanNgram)
        .unwrap();
    assert_eq!(
        c.search_keywords("백업", 10, filter)
            .unwrap()
            .neighbors
            .len(),
        1
    );
}

#[test]
fn worker_exposes_all_retrieval_modes_and_maintains_one_owner_through_backup() {
    let t = TempDir::new();
    let mut c = Collection::create(t.path(), "notes", Config::new(2), "fixture").unwrap();
    let mut doc = Document::new(u64::MAX, "백업을 복원합니다", "recovery.md");
    doc.tags = vec!["keep".into()];
    c.put(&doc, &[1.0, 0.0]).unwrap();
    c.put(&Document::new(1, "백업을 secret", "hidden.md"), &[0.0, 0.0])
        .unwrap();
    c.set_keyword_analyzer(KeywordAnalyzer::KoreanNgram)
        .unwrap();
    let space = c.space().clone();
    let (client, worker) = app::start(c).unwrap();
    let mut filter = app::QueryFilter::default();
    filter.expression = Some(FilterExpr::tag("keep"));
    assert_eq!(
        vecnook::doctor(t.path().join("notes")).unwrap_err().kind(),
        ErrorKind::Locked
    );
    let lexical = client
        .submit_keywords("fixture", "백업".into(), 10, filter.clone())
        .unwrap();
    let hybrid = client
        .submit_hybrid(
            "fixture",
            vec![1.0, 0.0],
            "백업".into(),
            10,
            HybridSearchOptions::default(),
            filter,
        )
        .unwrap();
    assert_eq!(
        lexical.recv().unwrap().unwrap().neighbors[0].document.id,
        u64::MAX
    );
    assert_eq!(
        hybrid
            .recv()
            .unwrap()
            .unwrap()
            .neighbors
            .iter()
            .map(|n| n.document.id)
            .collect::<Vec<_>>(),
        [u64::MAX]
    );
    let status = client
        .status(MaintenancePolicy::default())
        .unwrap()
        .recv()
        .unwrap()
        .unwrap();
    assert_eq!(status.capacity.active_records, 2);
    let mut replacement = doc.clone();
    replacement.text = "새로운 백업을 복원합니다".into();
    let written = client
        .submit_batch(
            "fixture",
            vec![app::WriteOperation::Put {
                document: replacement.clone(),
                vector: vec![1.0, 0.0],
            }],
            Some(status.sequence),
        )
        .unwrap()
        .recv()
        .unwrap()
        .unwrap();
    assert!(written.sequence > status.sequence);
    let stale = client
        .submit_batch(
            "fixture",
            vec![app::WriteOperation::Delete { id: u64::MAX }],
            Some(status.sequence),
        )
        .unwrap();
    assert_eq!(
        stale.recv().unwrap().unwrap_err().kind(),
        ErrorKind::Conflict
    );
    client.compact().unwrap().recv().unwrap().unwrap();
    client
        .maintain(MaintenancePolicy::default())
        .unwrap()
        .recv()
        .unwrap()
        .unwrap();
    client
        .backup(t.path().into(), "backup".into())
        .unwrap()
        .recv()
        .unwrap()
        .unwrap();
    drop(client);
    worker.join().unwrap().check_invariants().unwrap();
    let backup = Collection::open(t.path(), "backup", &space).unwrap();
    assert_eq!(backup.get(u64::MAX).unwrap().unwrap(), replacement);
}

#[test]
fn inspection_keeps_an_incomplete_wal_byte_for_byte_and_obeys_the_lock() {
    use std::io::Write;
    let t = TempDir::new();
    let mut c = Collection::create(t.path(), "notes", Config::new(1), "fixture").unwrap();
    c.put(&Document::new(7, "original", "notes.md"), &[0.25])
        .unwrap();
    assert_eq!(
        vecnook::inspect_documents(t.path().join("notes"))
            .unwrap_err()
            .kind(),
        ErrorKind::Locked
    );
    drop(c);
    let path = t.path().join("notes/wal.bin");
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(&[1, 2, 3])
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    let inspected = vecnook::inspect_documents(t.path().join("notes")).unwrap();
    assert_eq!(inspected.diagnostics.pending_tail_bytes, 3);
    assert_eq!(inspected.documents[0].0.text, "original");
    assert_eq!(inspected.documents[0].1, [0.25]);
    assert_eq!(std::fs::read(path).unwrap(), before);
}
