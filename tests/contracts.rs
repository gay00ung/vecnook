mod support;
use support::TempDir;
use vecnook::{
    Collection, Config, Database, EmbeddingSpace, Error, ErrorKind, Metric, Mutation,
    SearchOptions, SearchStrategy, VectorIndex,
};

#[test]
fn downstream_builders_and_error_categories_support_application_decisions() {
    let config = Config::new(2)
        .with_m(8)
        .with_ef_construction(32)
        .with_seed(7)
        .with_metric(Metric::Cosine);
    config.validate().unwrap();
    let mut index = VectorIndex::new(config.clone()).unwrap();
    let options = SearchOptions::default()
        .with_strategy(SearchStrategy::Exact)
        .with_ef_search(0)
        .with_exact_threshold(0);
    options.validate().unwrap();
    index.put(u64::MAX, &[1.0, 0.0], "한글\n").unwrap();
    assert_eq!(
        index.search(&[1.0, 0.0], 1, options).unwrap().neighbors[0].id,
        u64::MAX
    );
    assert_eq!(
        index.put(0, &[f32::NAN, 0.0], "").unwrap_err().kind(),
        ErrorKind::InvalidInput
    );
    let capacity_dir = TempDir::new();
    let mut db = Database::create(capacity_dir.path(), config.clone()).unwrap();
    db.put(u64::MAX, &[1.0, 0.0], "").unwrap();
    let too_many = vec![Mutation::Delete { id: u64::MAX }; 1025];
    assert!(matches!(
        db.write_batch(&too_many),
        Err(Error::Capacity {
            resource: "batch_operations",
            limit: 1024,
            required: 1025
        })
    ));
    assert_eq!(index.len(), 1);
    let tmp = TempDir::new();
    let collection = Collection::create(tmp.path(), "notes", config, "v1").unwrap();
    assert_eq!(
        Collection::open(
            tmp.path(),
            "notes",
            &EmbeddingSpace::new("v2", 2, Metric::Cosine)
        )
        .err()
        .unwrap()
        .kind(),
        ErrorKind::EmbeddingMismatch
    );
    assert_eq!(
        Collection::open(tmp.path(), "notes", collection.space())
            .err()
            .unwrap()
            .kind(),
        ErrorKind::Locked
    );
    let original = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
    let error = Error::from(original);
    assert_eq!(error.kind(), ErrorKind::Io);
    assert!(std::error::Error::source(&error).is_some());
}
