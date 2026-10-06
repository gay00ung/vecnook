use vecnook::{
    Config, Metric, SearchMode, SearchOptions, SearchReason, SearchStrategy, VectorIndex,
};

fn options(strategy: SearchStrategy) -> SearchOptions {
    SearchOptions::default().with_strategy(strategy)
}

#[test]
fn cosine_is_scale_invariant_and_preserves_original_vectors() {
    let mut index = VectorIndex::new(Config::new(2).with_metric(Metric::Cosine)).unwrap();
    for (id, v) in [(1, [3.0, 4.0]), (2, [-4.0, 3.0]), (3, [-3.0, -4.0])] {
        index.put(id, &v, "").unwrap();
    }
    let found = index.search_exact(&[6.0, 8.0], 3).unwrap();
    assert_eq!(found.metric, Metric::Cosine);
    for (neighbor, expected) in found.neighbors.iter().zip([0.0, 1.0, 2.0]) {
        assert!((neighbor.distance - expected).abs() < 1e-14);
    }
    assert_eq!(index.get(1).unwrap().vector, [3.0, 4.0]);
    assert_eq!(
        found.neighbors,
        index.search_hnsw(&[12.0, 16.0], 3, 128).unwrap().neighbors
    );
}

#[test]
fn inner_product_sorts_highest_similarity_first_including_negative_scores() {
    let mut index = VectorIndex::new(Config::new(2).with_metric(Metric::InnerProduct)).unwrap();
    for (id, v) in [(1, [1.0, 0.0]), (2, [10.0, 0.0]), (3, [-1.0, 0.0])] {
        index.put(id, &v, "").unwrap();
    }
    let found = index.search_exact(&[2.0, 0.0], 3).unwrap();
    assert_eq!(
        found.neighbors.iter().map(|n| n.id).collect::<Vec<_>>(),
        [2, 1, 3]
    );
    assert_eq!(
        found
            .neighbors
            .iter()
            .map(|n| n.distance)
            .collect::<Vec<_>>(),
        [-20.0, -2.0, 2.0]
    );
    assert_eq!(
        found.neighbors,
        index.search_hnsw(&[2.0, 0.0], 3, 128).unwrap().neighbors
    );
}

#[test]
fn cosine_zero_vectors_are_rejected_before_mutation_or_predicate_evaluation() {
    let mut index = VectorIndex::new(Config::new(2).with_metric(Metric::Cosine)).unwrap();
    assert!(index.put(1, &[0.0, -0.0], "").is_err());
    assert!(index.is_empty());
    let mut calls = 0;
    assert!(
        index
            .search_filtered(&[0.0, 0.0], 0, SearchOptions::default(), |_| {
                calls += 1;
                true
            })
            .is_err()
    );
    assert_eq!(calls, 0);
}

#[test]
fn zero_vectors_remain_valid_for_l2_and_inner_product() {
    for metric in [Metric::SquaredL2, Metric::InnerProduct] {
        let mut index = VectorIndex::new(Config::new(2).with_metric(metric)).unwrap();
        index.put(1, &[0.0, 0.0], "").unwrap();
        assert_eq!(
            index.search_exact(&[0.0, 0.0], 1).unwrap().neighbors[0].distance,
            0.0
        );
    }
}

#[test]
fn cosine_extreme_finite_and_subnormal_coordinates_have_finite_distances() {
    let mut index = VectorIndex::new(Config::new(2).with_metric(Metric::Cosine)).unwrap();
    index.put(1, &[f32::MAX, f32::MAX], "large").unwrap();
    index
        .put(2, &[f32::from_bits(1), f32::from_bits(1)], "small")
        .unwrap();
    for neighbor in index.search_exact(&[1.0, 1.0], 2).unwrap().neighbors {
        assert!(neighbor.distance.is_finite() && neighbor.distance < 1e-14);
    }
}

fn oracle(metric: Metric, a: &[f32], b: &[f32]) -> f64 {
    let mut dot = 0.0;
    let mut aa = 0.0;
    let mut bb = 0.0;
    let mut l2 = 0.0;
    for i in 0..a.len() {
        let x = f64::from(a[i]);
        let y = f64::from(b[i]);
        dot += x * y;
        aa += x * x;
        bb += y * y;
        l2 += (x - y) * (x - y);
    }
    match metric {
        Metric::SquaredL2 => l2,
        Metric::InnerProduct => -dot,
        Metric::Cosine => (1.0 - dot / (aa.sqrt() * bb.sqrt())).clamp(0.0, 2.0),
        _ => panic!("oracle only covers the three fixture metrics"),
    }
}

#[test]
fn all_metrics_match_independent_sorted_distance_oracle() {
    for metric in [Metric::SquaredL2, Metric::Cosine, Metric::InnerProduct] {
        let mut index = VectorIndex::new(Config::new(3).with_metric(metric)).unwrap();
        let corpus: Vec<_> = (0..150)
            .map(|i| {
                [
                    ((i * 17) % 113) as f32 - 50.0,
                    ((i * 37) % 97) as f32 - 40.0,
                    1.0,
                ]
            })
            .collect();
        for (id, v) in corpus.iter().enumerate() {
            index.put(id as u64, v, "").unwrap();
        }
        let query = [2.3, -1.7, 1.0];
        let mut expected: Vec<_> = corpus
            .iter()
            .enumerate()
            .map(|(id, v)| (oracle(metric, &query, v), id as u64))
            .collect();
        expected.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let found = index.search_exact(&query, 20).unwrap();
        for (actual, &(distance, id)) in found.neighbors.iter().zip(&expected) {
            assert_eq!(actual.id, id);
            assert!((actual.distance - distance).abs() < 1e-10);
        }
        assert_eq!(
            found.neighbors,
            index.search_hnsw(&query, 20, 4096).unwrap().neighbors
        );
        index.check_invariants().unwrap();
    }
}

#[test]
fn narrow_filter_is_exact_and_does_not_drop_matches_after_top_k() {
    let mut index = VectorIndex::new(Config::new(2)).unwrap();
    for id in 0..500 {
        index
            .put(
                id,
                &[id as f32, 0.0],
                if id >= 495 { "tenant-b" } else { "tenant-a" },
            )
            .unwrap();
    }
    let found = index
        .search_filtered(&[0.0, 0.0], 10, SearchOptions::default(), |r| {
            r.metadata == "tenant-b"
        })
        .unwrap();
    assert_eq!(found.mode, SearchMode::Exact);
    assert_eq!(found.reason, SearchReason::SmallEligibleSet);
    assert_eq!(found.eligible_count, 5);
    assert_eq!(found.distance_computations, 5);
    assert!(found.complete);
    assert_eq!(
        found.neighbors.iter().map(|n| n.id).collect::<Vec<_>>(),
        [495, 496, 497, 498, 499]
    );
}

#[test]
fn predicate_is_evaluated_once_per_active_record_and_never_for_tombstones() {
    let mut index = VectorIndex::new(Config::new(1)).unwrap();
    index.put(1, &[1.0], "old").unwrap();
    index.put(1, &[2.0], "new").unwrap();
    index.put(2, &[3.0], "deleted").unwrap();
    index.delete(2);
    let mut calls = 0;
    let found = index
        .search_filtered(&[0.0], 5, SearchOptions::default(), |r| {
            calls += 1;
            assert_eq!(r.metadata, "new");
            true
        })
        .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(found.eligible_count, 1);
    assert_eq!(found.neighbors[0].id, 1);
}

#[test]
fn hnsw_can_cross_filter_rejected_nodes() {
    let mut index = VectorIndex::new(Config::new(2)).unwrap();
    for id in 0..300 {
        index
            .put(
                id,
                &[id as f32, 1.0],
                if id % 37 == 0 { "allowed" } else { "bridge" },
            )
            .unwrap();
    }
    let found = index
        .search_filtered(
            &[110.0, 1.0],
            5,
            options(SearchStrategy::Hnsw).with_ef_search(4096),
            |r| r.metadata == "allowed",
        )
        .unwrap();
    let exact = index
        .search_filtered(&[110.0, 1.0], 5, options(SearchStrategy::Exact), |r| {
            r.metadata == "allowed"
        })
        .unwrap();
    assert_eq!(found.mode, SearchMode::Hnsw);
    assert_eq!(found.reason, SearchReason::HnswRequested);
    assert_eq!(found.neighbors, exact.neighbors);
    assert_eq!(found.eligible_count, 9);
}

#[test]
fn auto_explains_graph_choice_and_large_k_exact_choice() {
    let mut index = VectorIndex::new(Config::new(2)).unwrap();
    for id in 0..400 {
        index.put(id, &[id as f32, 1.0], "").unwrap();
    }
    let ann = index
        .search(&[1.0, 1.0], 10, SearchOptions::default())
        .unwrap();
    assert_eq!(ann.mode, SearchMode::Hnsw);
    assert_eq!(ann.reason, SearchReason::GraphSelected);
    let all = index
        .search(&[1.0, 1.0], 400, SearchOptions::default())
        .unwrap();
    assert_eq!(all.mode, SearchMode::Exact);
    assert_eq!(all.reason, SearchReason::LargeK);
    assert_eq!(all.neighbors.len(), 400);
}

#[test]
fn filtered_empty_and_zero_k_have_defined_results() {
    let mut index = VectorIndex::new(Config::new(1)).unwrap();
    index.put(1, &[1.0], "").unwrap();
    for strategy in [
        SearchStrategy::Exact,
        SearchStrategy::Hnsw,
        SearchStrategy::Auto,
    ] {
        let none = index
            .search_filtered(&[0.0], 10, options(strategy), |_| false)
            .unwrap();
        assert!(none.complete && none.neighbors.is_empty());
        assert_eq!(none.eligible_count, 0);
        let zero = index
            .search_filtered(&[0.0], 0, options(strategy), |_| true)
            .unwrap();
        assert!(zero.complete && zero.neighbors.is_empty());
        assert_eq!(zero.distance_computations, 0);
    }
}

#[test]
fn filtered_hnsw_validates_ef_against_eligible_count_not_whole_database() {
    let mut index = VectorIndex::new(Config::new(1)).unwrap();
    for id in 0..20 {
        index.put(id, &[id as f32], "").unwrap();
    }
    let small = SearchOptions::default()
        .with_strategy(SearchStrategy::Hnsw)
        .with_ef_search(2)
        .with_exact_threshold(0);
    assert!(
        index
            .search_filtered(&[0.0], 10, small, |r| r.id < 2)
            .is_ok()
    );
    assert!(
        index
            .search_filtered(&[0.0], 10, small, |r| r.id < 3)
            .is_err()
    );
    for ef in [0, 4097] {
        assert!(
            index
                .search(&[0.0], 1, SearchOptions::default().with_ef_search(ef))
                .is_err()
        );
    }
}

#[test]
fn unicode_metadata_and_id_range_predicates_keep_distance_id_order() {
    let mut index = VectorIndex::new(Config::new(1)).unwrap();
    for id in [u64::MAX, 0, 10, 20] {
        index.put(id, &[1.0], "문서:고양이 🐈").unwrap();
    }
    let found = index
        .search_filtered(&[0.0], 10, SearchOptions::default(), |r| {
            r.metadata.starts_with("문서:") && r.id >= 10
        })
        .unwrap();
    assert_eq!(
        found.neighbors.iter().map(|n| n.id).collect::<Vec<_>>(),
        [10, 20, u64::MAX]
    );
}
