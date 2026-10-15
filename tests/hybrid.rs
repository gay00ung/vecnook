mod support;
use support::TempDir;
use vecnook::{
    Collection, Config, Document, DocumentFilter, DocumentMutation, HybridSearchOptions,
    SearchOptions, SearchStrategy,
};

fn create(temp: &TempDir) -> Collection {
    Collection::create(temp.path(), "notes", Config::new(2), "fixed-fixture-v1").unwrap()
}
fn ids(report: &vecnook::KeywordSearchReport) -> Vec<u64> {
    report.neighbors.iter().map(|n| n.document.id).collect()
}

#[test]
fn bm25_matches_independent_counts_and_filters_preserve_corpus_statistics() {
    let temp = TempDir::new();
    let mut c = create(&temp);
    for (id, text) in [
        (1, "alpha alpha beta"),
        (2, "alpha beta gamma gamma"),
        (3, "delta"),
        (4, ""),
    ] {
        c.put(
            &Document::new(id, text, format!("{id}.md")),
            &[id as f32, 1.0],
        )
        .unwrap();
    }
    let found = c
        .search_keywords("ALPHA beta alpha", 10, DocumentFilter::default())
        .unwrap();
    assert_eq!(ids(&found), [1, 2]);
    assert_eq!(
        (
            found.eligible_count,
            found.matching_count,
            found.postings_visited
        ),
        (4, 2, 4)
    );
    // N=4 (including the empty text), total length=8, average=2, df=2
    // for both terms. Counts are specified independently of the tokenizer.
    let idf = 2f64.ln();
    let bm25 = |tf: f64, length: f64| idf * tf * 2.2 / (tf + 1.2 * (0.25 + 0.75 * length / 2.0));
    assert!((found.neighbors[0].score - (bm25(2.0, 3.0) + bm25(1.0, 3.0))).abs() < 1e-12);
    assert!((found.neighbors[1].score - 2.0 * bm25(1.0, 4.0)).abs() < 1e-12);
    let filtered = c
        .search_keywords(
            "alpha beta",
            10,
            DocumentFilter::default().with_source("2.md"),
        )
        .unwrap();
    assert_eq!(ids(&filtered), [2]);
    assert_eq!(filtered.eligible_count, 1);
    assert_eq!(filtered.neighbors[0].score, found.neighbors[1].score);
    assert!(
        c.search_keywords("alp", 10, DocumentFilter::default())
            .unwrap()
            .neighbors
            .is_empty()
    );
}

#[test]
fn identifiers_unicode_empty_queries_ties_and_limits_are_defined() {
    let temp = TempDir::new();
    let mut c = create(&temp);
    for id in [u64::MAX, 7] {
        c.put(
            &Document::new(id, "search_metadata ERR-42 Vecnook 백업 한글", "same.md"),
            &[1.0, 0.0],
        )
        .unwrap();
    }
    for query in [
        "SEARCH_METADATA",
        "err-42",
        "vecnook",
        "백업",
        "한글",
        "search_metadata search_metadata",
    ] {
        let found = c
            .search_keywords(query, 20, DocumentFilter::default())
            .unwrap();
        assert_eq!(ids(&found), [7, u64::MAX]);
        assert!(
            found
                .neighbors
                .iter()
                .all(|n| n.score.is_finite() && n.score > 0.0)
        );
    }
    for query in ["", "... -- ___", "absent", "백업을"] {
        assert!(
            c.search_keywords(query, 10, DocumentFilter::default())
                .unwrap()
                .neighbors
                .is_empty()
        );
    }
    let zero = c
        .search_keywords("한글", 0, DocumentFilter::default())
        .unwrap();
    assert!(zero.neighbors.is_empty());
    assert_eq!(zero.matching_count, 2);
    assert!(
        c.search_keywords(&"a".repeat(6001), 1, DocumentFilter::default())
            .is_err()
    );
    assert!(
        c.search_keywords("a\0b", 1, DocumentFilter::default())
            .is_err()
    );
    assert!(
        c.search_keywords("a", 1, DocumentFilter::default().with_tags(&["x"; 33]))
            .is_err()
    );
}

#[test]
fn fusion_uses_one_based_ranks_deduplicates_and_does_not_leak_filtered_documents() {
    let temp = TempDir::new();
    let mut c = create(&temp);
    for (id, text, vector, tag) in [
        (1, "ordinary", [0.0, 0.0], "keep"),
        (2, "ERR-42", [1.0, 0.0], "keep"),
        (3, "ERR-42 ERR-42", [2.0, 0.0], "keep"),
        (4, "ERR-42 ERR-42 ERR-42", [0.0, 0.0], "private"),
    ] {
        let mut document = Document::new(id, text, "errors.md");
        document.tags = vec![tag.into()];
        c.put(&document, &vector).unwrap();
    }
    let filter = DocumentFilter::default().with_tags(&["keep"]);
    let options = HybridSearchOptions::default().with_candidates(3);
    let report = c
        .search_hybrid(&[0.0, 0.0], "ERR-42", 3, options, filter)
        .unwrap();
    assert_eq!(report.candidate_count, 3);
    assert!(report.complete);
    assert_eq!(report.vector.eligible_count, 3);
    assert_eq!(report.keyword.eligible_count, 3);
    assert_eq!(
        report
            .neighbors
            .iter()
            .map(|n| n.document.id)
            .collect::<Vec<_>>(),
        [3, 2, 1]
    );
    for neighbor in &report.neighbors {
        let expected = neighbor
            .vector_rank
            .map_or(0.0, |rank| 1.0 / (60.0 + rank as f64))
            + neighbor
                .keyword_rank
                .map_or(0.0, |rank| 1.0 / (60.0 + rank as f64));
        assert_eq!(neighbor.score, expected);
        assert_ne!(neighbor.document.id, 4);
    }
    // Absent tokens preserve the vector branch's distance/ID ordering.
    let fallback = c
        .search_hybrid(&[0.0, 0.0], "unknown", 3, options, filter)
        .unwrap();
    assert_eq!(
        fallback
            .neighbors
            .iter()
            .map(|n| n.document.id)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    let missing = c
        .search_hybrid(
            &[0.0, 0.0],
            "ERR-42",
            3,
            options,
            DocumentFilter::default().with_tags(&["absent"]),
        )
        .unwrap();
    assert!(missing.complete);
    assert!(missing.neighbors.is_empty());
    let zero = c
        .search_hybrid(&[0.0, 0.0], "ERR-42", 0, options, filter)
        .unwrap();
    assert!(zero.complete);
    assert_eq!(zero.candidate_count, 0);
    assert!(
        c.search_hybrid(&[0.0], "ERR-42", 0, options, filter)
            .is_err()
    );
    assert!(
        HybridSearchOptions::default()
            .with_candidates(0)
            .validate(1)
            .is_err()
    );
    assert!(
        HybridSearchOptions::default()
            .with_rrf_k(0)
            .validate(1)
            .is_err()
    );
    assert!(HybridSearchOptions::default().validate(65).is_err());
    assert!(
        HybridSearchOptions::default()
            .with_vector_options(
                SearchOptions::default()
                    .with_strategy(SearchStrategy::Hnsw)
                    .with_ef_search(0)
            )
            .validate(1)
            .is_err()
    );
}

#[test]
fn ordered_mutations_failed_batches_restart_compaction_backup_and_export_rebuild_keywords() {
    let temp = TempDir::new();
    let mut c = create(&temp);
    let old = Document::new(7, "oldword oldword", "old.md");
    let new = Document::new(7, "newword 백업", "new.md");
    c.put(&old, &[1.0, 0.0]).unwrap();
    let sequence = c.sequence();
    assert!(
        c.write_batch(&[
            DocumentMutation::Delete { id: 7 },
            DocumentMutation::Put {
                document: &new,
                vector: &[f32::NAN, 0.0]
            },
        ])
        .is_err()
    );
    assert_eq!(c.sequence(), sequence);
    assert_eq!(
        ids(&c
            .search_keywords("oldword", 10, DocumentFilter::default())
            .unwrap()),
        [7]
    );
    c.write_batch(&[
        DocumentMutation::Put {
            document: &new,
            vector: &[0.0, 1.0],
        },
        DocumentMutation::Delete { id: 7 },
        DocumentMutation::Put {
            document: &new,
            vector: &[1.0, 0.0],
        },
        DocumentMutation::Delete { id: 99 },
    ])
    .unwrap();
    let space = c.space().clone();
    drop(c); // Rebuild from WAL, without a checkpoint.
    let mut c = Collection::open(temp.path(), "notes", &space).unwrap();
    c.check_invariants().unwrap();
    assert!(
        c.search_keywords("oldword", 10, DocumentFilter::default())
            .unwrap()
            .neighbors
            .is_empty()
    );
    let expected = c
        .search_hybrid(
            &[1.0, 0.0],
            "newword",
            10,
            HybridSearchOptions::default(),
            DocumentFilter::default(),
        )
        .unwrap()
        .neighbors;
    c.compact().unwrap();
    c.backup(temp.path(), "copy").unwrap();
    let export = temp.path().join("documents.export");
    c.export(&export).unwrap();
    drop(c);
    let restored = Collection::import(&export, temp.path(), "restored").unwrap();
    restored.check_invariants().unwrap();
    drop(restored);
    for name in ["notes", "copy", "restored"] {
        let mut copy = Collection::open(temp.path(), name, &space).unwrap();
        copy.check_invariants().unwrap();
        assert_eq!(
            copy.search_hybrid(
                &[1.0, 0.0],
                "newword",
                10,
                HybridSearchOptions::default(),
                DocumentFilter::default()
            )
            .unwrap()
            .neighbors,
            expected
        );
        copy.delete(7).unwrap();
        copy.check_invariants().unwrap();
        assert!(
            copy.search_keywords("newword", 10, DocumentFilter::default())
                .unwrap()
                .neighbors
                .is_empty()
        );
    }
}
