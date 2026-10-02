use std::sync::Arc;
use vector::{Config, Error, SearchMode, VectorIndex};

fn index(dimensions: usize) -> VectorIndex {
    VectorIndex::new(Config::new(dimensions)).unwrap()
}
fn ids(report: vector::SearchReport) -> Vec<u64> {
    report.neighbors.into_iter().map(|n| n.id).collect()
}

#[test]
fn exact_search_matches_hand_computed_distances_and_ties() {
    let mut db = index(2);
    db.put(30, &[3.0, 4.0], "third").unwrap();
    db.put(10, &[0.0, 0.0], "first").unwrap();
    db.put(20, &[-3.0, -4.0], "second").unwrap();
    let found = db.search_exact(&[0.0, 0.0], 3).unwrap();
    assert_eq!(found.mode, SearchMode::Exact);
    assert_eq!(found.distance_computations, 3);
    assert!(found.complete);
    assert_eq!(
        found
            .neighbors
            .iter()
            .map(|n| (n.id, n.distance))
            .collect::<Vec<_>>(),
        [(10, 0.0), (20, 25.0), (30, 25.0)]
    );
    assert_eq!(found.neighbors[1].metadata, "second");
}

#[test]
fn exact_top_k_matches_independently_sorted_oracle() {
    let mut db = index(3);
    let values: Vec<_> = (0..180)
        .map(|i| {
            [
                ((i * 37) % 101) as f32,
                ((i * 53) % 107) as f32,
                ((i * 71) % 109) as f32,
            ]
        })
        .collect();
    for (i, value) in values.iter().enumerate() {
        db.put(i as u64, value, "").unwrap();
    }
    let query = [13.5, 47.25, -2.0];
    let mut oracle: Vec<_> = values
        .iter()
        .enumerate()
        .map(|(id, v)| {
            let a = f64::from(v[0]) - f64::from(query[0]);
            let b = f64::from(v[1]) - f64::from(query[1]);
            let c = f64::from(v[2]) - f64::from(query[2]);
            (id as u64, a * a + b * b + c * c)
        })
        .collect();
    oracle.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    let found = db.search_exact(&query, 17).unwrap();
    assert_eq!(
        found
            .neighbors
            .iter()
            .map(|n| (n.id, n.distance))
            .collect::<Vec<_>>(),
        oracle[..17]
    );
}

#[test]
fn finite_extreme_coordinates_do_not_overflow_distance() {
    let mut db = index(2);
    db.put(1, &[f32::MAX, -f32::MAX], "").unwrap();
    let found = db.search_exact(&[-f32::MAX, f32::MAX], 1).unwrap();
    assert!(found.neighbors[0].distance.is_finite());
    assert!(found.neighbors[0].distance > 1e77);
    assert_eq!(
        found.neighbors,
        db.search_hnsw(&[-f32::MAX, f32::MAX], 1, 10)
            .unwrap()
            .neighbors
    );
}

#[test]
fn bad_registration_does_not_change_state() {
    let mut db = index(2);
    db.put(1, &[1.0, 2.0], "keep").unwrap();
    for vector in [
        vec![1.0],
        vec![f32::NAN, 0.0],
        vec![f32::INFINITY, 0.0],
        vec![f32::NEG_INFINITY, 0.0],
    ] {
        assert!(matches!(
            db.put(1, &vector, "replace"),
            Err(Error::InvalidInput(_))
        ));
    }
    assert_eq!(db.stats().physical_nodes, 1);
    assert_eq!(db.get(1).unwrap().metadata, "keep");
}

#[test]
fn query_validation_also_applies_to_empty_and_zero_k_searches() {
    let db = index(2);
    assert!(db.search_exact(&[0.0], 0).is_err());
    assert!(db.search_hnsw(&[f32::NAN, 0.0], 0, 128).is_err());
    assert!(db.search_hnsw(&[0.0, 0.0], 0, 0).is_err());
}

#[test]
fn empty_zero_k_and_oversized_k_have_defined_results() {
    let mut db = index(2);
    assert!(
        db.search_hnsw(&[0.0, 0.0], 10, 128)
            .unwrap()
            .neighbors
            .is_empty()
    );
    db.put(1, &[0.0, 0.0], "").unwrap();
    db.put(u64::MAX, &[1.0, 0.0], "").unwrap();
    assert!(
        db.search_exact(&[0.0, 0.0], 0)
            .unwrap()
            .neighbors
            .is_empty()
    );
    assert!(
        db.search_hnsw(&[0.0, 0.0], 0, 1)
            .unwrap()
            .neighbors
            .is_empty()
    );
    assert_eq!(
        ids(db.search_exact(&[0.0, 0.0], usize::MAX).unwrap()),
        [1, u64::MAX]
    );
    assert_eq!(
        ids(db.search_hnsw(&[0.0, 0.0], usize::MAX, 2).unwrap()),
        [1, u64::MAX]
    );
}

#[test]
fn metadata_limit_is_measured_in_utf8_bytes() {
    let mut db = index(1);
    db.put(1, &[1.0], &"a".repeat(16 * 1024)).unwrap();
    assert!(db.put(2, &[2.0], &"a".repeat(16 * 1024 + 1)).is_err());
    assert!(db.put(3, &[3.0], &"한".repeat(5462)).is_err());
    db.put(4, &[4.0], "한글 metadata 🐈").unwrap();
    assert_eq!(db.get(4).unwrap().metadata, "한글 metadata 🐈");
    assert_eq!(db.len(), 2);
}

#[test]
fn same_id_update_replaces_coordinates_and_metadata() {
    let mut db = index(2);
    assert!(db.put(1, &[0.0, 0.0], "old").unwrap());
    db.put(2, &[1.0, 1.0], "other").unwrap();
    assert!(!db.put(1, &[100.0, 100.0], "new").unwrap());
    assert_eq!(db.len(), 2);
    assert_eq!(db.stats().tombstones, 1);
    let found = db.search_hnsw(&[0.0, 0.0], 2, 16).unwrap();
    assert_eq!(found.neighbors[0].id, 2);
    assert_eq!(found.neighbors[1].metadata, "new");
    assert_eq!(found.neighbors[1].distance, 20_000.0);
    db.check_invariants().unwrap();
}

#[test]
fn deleting_all_but_one_preserves_traversal_through_tombstones() {
    let mut db = index(2);
    for i in 0..300 {
        db.put(i, &[i as f32, (i % 13) as f32], "").unwrap();
    }
    // The highest-layer entry is overwhelmingly among the deleted nodes;
    // traversal must still reach the remaining live record.
    for i in 0..299 {
        assert!(db.delete(i));
    }
    let found = db.search_hnsw(&[0.0, 0.0], 10, 16).unwrap();
    assert!(found.complete);
    assert_eq!(ids(found), [299]);
    db.check_invariants().unwrap();
}

#[test]
fn delete_all_then_reuse_an_id() {
    let mut db = index(2);
    for id in 0..30 {
        db.put(id, &[id as f32, 0.0], "").unwrap();
    }
    for id in 0..30 {
        assert!(db.delete(id));
    }
    assert!(!db.delete(30));
    assert!(
        db.search_exact(&[0.0, 0.0], 10)
            .unwrap()
            .neighbors
            .is_empty()
    );
    assert!(
        db.search_hnsw(&[0.0, 0.0], 10, 10)
            .unwrap()
            .neighbors
            .is_empty()
    );
    assert!(db.put(5, &[1000.0, 1.0], "reborn").unwrap());
    let found = db.search_hnsw(&[1000.0, 1.0], 10, 10).unwrap();
    assert_eq!(found.neighbors.len(), 1);
    assert_eq!(found.neighbors[0].metadata, "reborn");
    assert_eq!(found.neighbors[0].distance, 0.0);
}

#[test]
fn compact_reclaims_old_nodes_without_changing_live_data() {
    let mut db = index(2);
    for id in 0..100 {
        db.put(id, &[id as f32, 1.0], "original").unwrap();
    }
    for id in 0..40 {
        db.delete(id);
    }
    for id in 40..60 {
        db.put(id, &[id as f32, 2.0], "new").unwrap();
    }
    let before = db.search_exact(&[50.25, 2.0], 10).unwrap().neighbors;
    assert_eq!(db.compact().unwrap(), 60);
    assert_eq!(db.stats().tombstones, 0);
    assert_eq!(db.stats().physical_nodes, 60);
    assert_eq!(
        db.search_exact(&[50.25, 2.0], 10).unwrap().neighbors,
        before
    );
    assert_eq!(
        db.search_hnsw(&[50.25, 2.0], 10, 128).unwrap().neighbors,
        before
    );
    db.check_invariants().unwrap();
}

#[test]
fn duplicate_coordinates_are_sorted_by_id() {
    let mut db = index(2);
    for id in [200, 100, 2, 77] {
        db.put(id, &[3.0, 4.0], "same").unwrap();
    }
    assert_eq!(ids(db.search_exact(&[3.0, 4.0], 3).unwrap()), [2, 77, 100]);
    assert_eq!(
        ids(db.search_hnsw(&[3.0, 4.0], 3, 64).unwrap()),
        [2, 77, 100]
    );
}

#[test]
fn ef_boundaries_are_checked_against_effective_k() {
    let mut db = index(1);
    for id in 0..10 {
        db.put(id, &[id as f32], "").unwrap();
    }
    assert!(db.search_hnsw(&[0.0], 10, 9).is_err());
    assert!(db.search_hnsw(&[0.0], 1, 4097).is_err());
    assert_eq!(db.search_hnsw(&[0.0], 100, 10).unwrap().neighbors.len(), 10);
}

#[test]
fn invalid_configurations_are_rejected() {
    for config in [
        Config::new(0),
        Config::new(4097),
        Config {
            m: 1,
            ..Config::new(2)
        },
        Config {
            m: 65,
            ..Config::new(2)
        },
        Config {
            m: 16,
            ef_construction: 15,
            ..Config::new(2)
        },
        Config {
            ef_construction: 4097,
            ..Config::new(2)
        },
    ] {
        assert!(VectorIndex::new(config).is_err());
    }
}

#[test]
fn valid_parameter_endpoints_accept_registration_and_search() {
    for config in [
        Config {
            dimensions: 1,
            m: 2,
            ef_construction: 2,
            seed: 42,
        },
        Config {
            dimensions: 4096,
            m: 64,
            ef_construction: 4096,
            seed: 42,
        },
    ] {
        let dimensions = config.dimensions;
        let mut db = VectorIndex::new(config).unwrap();
        let zeros = vec![0.0; dimensions];
        let mut other = zeros.clone();
        other[0] = 1.0;
        db.put(0, &zeros, "zero").unwrap();
        db.put(u64::MAX, &other, "one").unwrap();
        assert_eq!(ids(db.search_hnsw(&zeros, 1, 1).unwrap()), [0]);
        assert_eq!(ids(db.search_hnsw(&zeros, 2, 4096).unwrap()), [0, u64::MAX]);
        db.check_invariants().unwrap();
    }
}

#[test]
fn multiple_layers_edges_and_deterministic_search_are_constructed() {
    let mut first = index(3);
    let mut second = index(3);
    for id in 0..500 {
        let value = [(id % 37) as f32, (id % 43) as f32, (id % 47) as f32];
        first.put(id, &value, "").unwrap();
        second.put(id, &value, "").unwrap();
    }
    first.check_invariants().unwrap();
    second.check_invariants().unwrap();
    assert!(first.stats().layers > 1);
    assert!(first.stats().directed_edges > 500);
    assert_eq!(first.stats().directed_edges, second.stats().directed_edges);
    for query in [[1.2, 8.3, 3.7], [36.8, 42.0, 46.0]] {
        let a = first.search_hnsw(&query, 10, 128).unwrap();
        let b = second.search_hnsw(&query, 10, 128).unwrap();
        assert_eq!(a.neighbors, b.neighbors);
        assert_eq!(a.distance_computations, b.distance_computations);
    }
}

#[test]
fn hnsw_performs_graph_search_without_hidden_exact_fallback() {
    let mut db = index(2);
    for id in 0..1500 {
        db.put(id, &[(id / 50) as f32 * 20.0, (id % 50) as f32 * 0.01], "")
            .unwrap();
    }
    let exact = db.search_exact(&[140.0, 0.233], 10).unwrap();
    let ann = db.search_hnsw(&[140.0, 0.233], 10, 32).unwrap();
    assert_eq!(ann.mode, SearchMode::Hnsw);
    assert_eq!(ann.neighbors, exact.neighbors);
    assert!(ann.distance_computations < exact.distance_computations);
}

#[test]
fn repeated_updates_and_deletions_keep_graph_invariants() {
    let mut db = index(3);
    for step in 0..800u64 {
        let id = (step * 37) % 90;
        if step % 5 == 0 {
            db.delete(id);
        } else {
            db.put(id, &[step as f32, id as f32, 1.0], "latest")
                .unwrap();
        }
        if step % 100 == 0 {
            db.check_invariants().unwrap();
        }
    }
    let found = db.search_hnsw(&[799.0, 1.0, 1.0], 10, 128).unwrap();
    let mut unique = std::collections::BTreeSet::new();
    for neighbor in found.neighbors {
        assert!(db.get(neighbor.id).is_some());
        assert!(unique.insert(neighbor.id));
    }
    db.compact().unwrap();
    db.check_invariants().unwrap();
}

#[test]
fn immutable_index_supports_concurrent_readers() {
    let mut db = index(2);
    for id in 0..100 {
        db.put(id, &[id as f32, 0.0], "").unwrap();
    }
    let db = Arc::new(db);
    let readers: Vec<_> = (0..8)
        .map(|_| {
            let db = Arc::clone(&db);
            std::thread::spawn(move || ids(db.search_hnsw(&[50.1, 0.0], 5, 64).unwrap()))
        })
        .collect();
    for reader in readers {
        assert_eq!(reader.join().unwrap(), [50, 51, 49, 52, 48]);
    }
}
