mod support;
use support::TempDir;
use vecnook::{Config, Database, Metric, Mutation, SearchOptions, SearchStrategy, VectorIndex};

fn exact() -> SearchOptions {
    SearchOptions {
        strategy: SearchStrategy::Exact,
        ..SearchOptions::default()
    }
}

#[test]
fn indexed_metadata_agrees_with_predicates_after_mutations_and_recovery() {
    for metric in [Metric::SquaredL2, Metric::Cosine, Metric::InnerProduct] {
        let temp = TempDir::new();
        let mut db = Database::create(temp.path(), Config::new(2).with_metric(metric)).unwrap();
        for id in 0..320 {
            db.put(
                id,
                &[id as f32 + 1.0, 2.0],
                if id % 4 == 0 {
                    "한글\nproject-a"
                } else {
                    "project-b"
                },
            )
            .unwrap();
        }
        db.write_batch(&[
            Mutation::Put {
                id: 4,
                vector: &[7.0, 8.0],
                metadata: "project-b",
            },
            Mutation::Delete { id: 8 },
            Mutation::Put {
                id: 8,
                vector: &[9.0, 3.0],
                metadata: "third",
            },
            Mutation::Delete { id: 8 },
            Mutation::Put {
                id: 1,
                vector: &[4.0, 6.0],
                metadata: "한글\nproject-a",
            },
        ])
        .unwrap();
        for phase in 0..4 {
            for label in ["한글\nproject-a", "project-b", "third", "missing", ""] {
                for options in [
                    exact(),
                    SearchOptions::default(),
                    SearchOptions {
                        strategy: SearchStrategy::Hnsw,
                        exact_threshold: 0,
                        ..SearchOptions::default()
                    },
                ] {
                    let indexed = db.search_metadata(&[1.0, 3.0], 10, options, label).unwrap();
                    let scanned = db
                        .search_filtered(&[1.0, 3.0], 10, options, |r| r.metadata == label)
                        .unwrap();
                    assert_eq!(
                        indexed.neighbors, scanned.neighbors,
                        "phase={phase}, metric={metric:?}"
                    );
                    assert_eq!(
                        (indexed.eligible_count, indexed.mode, indexed.reason),
                        (scanned.eligible_count, scanned.mode, scanned.reason)
                    );
                    assert_eq!(scanned.filter_evaluations, db.stats().active_records);
                    assert_eq!(indexed.filter_evaluations, indexed.eligible_count);
                    assert!(indexed.neighbors.iter().all(|n| n.metadata == label));
                }
            }
            db.check_invariants().unwrap();
            match phase {
                0 => {
                    drop(db);
                    db = Database::open(temp.path()).unwrap();
                }
                1 => {
                    db.checkpoint().unwrap();
                    drop(db);
                    db = Database::open(temp.path()).unwrap();
                    assert!(db.recovery_info().graph_cache_loaded);
                }
                2 => {
                    db.compact().unwrap();
                    drop(db);
                    db = Database::open(temp.path()).unwrap();
                }
                _ => (),
            }
        }
    }
}

#[test]
fn unfiltered_auto_uses_no_predicate_scan_and_preserves_results() {
    let mut index = VectorIndex::new(Config::new(2)).unwrap();
    for id in 0..600 {
        index.put(id, &[id as f32, 0.0], "").unwrap();
    }
    for k in [0, 1, 10, 600, 1000] {
        let options = SearchOptions::default();
        let fast = index.search(&[277.3, 1.0], k, options).unwrap();
        let scanned = index
            .search_filtered(&[277.3, 1.0], k, options, |_| true)
            .unwrap();
        assert_eq!(fast.neighbors, scanned.neighbors);
        assert_eq!(fast.reason, scanned.reason);
        assert_eq!(fast.filter_evaluations, 0);
        assert_eq!(scanned.filter_evaluations, 600);
    }
}

#[test]
fn id_subset_deduplicates_and_ignores_absent_and_deleted() {
    let mut index = VectorIndex::new(Config::new(1)).unwrap();
    for id in 0..20 {
        index.put(id, &[id as f32], "label").unwrap();
    }
    index.delete(3);
    let report = index
        .search_ids(&[0.0], 10, exact(), &[9, 3, 2, 2, u64::MAX, 7])
        .unwrap();
    assert_eq!(report.eligible_count, 3);
    assert_eq!(
        report.neighbors.iter().map(|n| n.id).collect::<Vec<_>>(),
        [2, 7, 9]
    );
    assert_eq!(report.distance_computations, 3);
    assert_eq!(report.filter_evaluations, 0);
    let empty = index
        .search_ids(&[0.0], 0, SearchOptions::default(), &[2, 7])
        .unwrap();
    assert!(empty.neighbors.is_empty() && empty.complete);
    assert!(
        index
            .search_ids(&[0.0], 1, exact(), &vec![1; 100_001])
            .is_err()
    );
    assert!(
        index
            .search_metadata(&[f32::NAN], 1, exact(), "missing")
            .is_err()
    );
}
