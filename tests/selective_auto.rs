use vecnook::{Config, SearchMode, SearchOptions, SearchReason, SearchStrategy, VectorIndex};

#[test]
fn selective_500_of_4000_uses_exact_and_forced_graph_remains_graph() {
    let mut index = VectorIndex::new(Config::new(1)).unwrap();
    for id in 0..4000 {
        index
            .put(
                id,
                &[id as f32],
                if id < 500 { "selected" } else { "other" },
            )
            .unwrap();
    }
    let options = SearchOptions::default();
    let query = [217.2];
    let exact = index
        .search_metadata(
            &query,
            10,
            options.with_strategy(SearchStrategy::Exact),
            "selected",
        )
        .unwrap();
    let ids: Vec<_> = (0..500).collect();
    for report in [
        index
            .search_metadata(&query, 10, options, "selected")
            .unwrap(),
        index.search_ids(&query, 10, options, &ids).unwrap(),
        index
            .search_filtered(&query, 10, options, |r| r.id < 500)
            .unwrap(),
    ] {
        assert_eq!(report.mode, SearchMode::Exact);
        assert_eq!(report.reason, SearchReason::SelectiveFilter);
        assert_eq!(report.distance_computations, 500);
        assert_eq!(report.neighbors, exact.neighbors);
        assert!(report.complete);
    }
    for options in [
        options.with_strategy(SearchStrategy::Hnsw),
        options.with_filtered_exact_threshold(0),
    ] {
        assert_eq!(
            index
                .search_metadata(&query, 10, options, "selected")
                .unwrap()
                .mode,
            SearchMode::Hnsw
        );
    }
    assert_eq!(
        index.search(&query, 10, options).unwrap().mode,
        SearchMode::Hnsw
    );
    assert_eq!(
        index
            .search_filtered(&query, 10, options, |r| r.id < 501)
            .unwrap()
            .mode,
        SearchMode::Hnsw
    );
    index.delete(3999); // The heuristic uses active count, not physical/tombstoned nodes.
    assert_eq!(
        index.search_ids(&query, 10, options, &ids).unwrap().mode,
        SearchMode::Hnsw
    );
}
