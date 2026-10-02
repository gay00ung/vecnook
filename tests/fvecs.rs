mod support;
use std::fs;
use support::TempDir;
use vecnook::{
    Metric,
    bench::{self, BenchConfig},
};

fn encode(vectors: &[&[f32]]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for vector in vectors {
        bytes.extend_from_slice(&(vector.len() as u32).to_le_bytes());
        for value in *vector {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

#[test]
fn fvecs_reads_bounded_prefix_and_preserves_coordinates() {
    let temp = TempDir::new();
    let path = temp.path().join("base.fvecs");
    fs::write(&path, encode(&[&[1.0, 2.0], &[3.0, 4.0]])).unwrap();
    assert_eq!(bench::read_fvecs(&path, 1).unwrap(), [vec![1.0, 2.0]]);
    assert_eq!(
        bench::read_fvecs(&path, 10).unwrap(),
        [vec![1.0, 2.0], vec![3.0, 4.0]]
    );
    assert!(bench::read_fvecs(&path, 0).is_err());
    assert!(bench::read_fvecs(&path, 100001).is_err());
}

#[test]
fn malformed_fvecs_headers_coordinates_and_dimensions_fail() {
    let temp = TempDir::new();
    let path = temp.path().join("bad.fvecs");
    for bytes in [
        vec![],
        vec![2, 0, 0],
        0u32.to_le_bytes().to_vec(),
        4097u32.to_le_bytes().to_vec(),
        encode(&[&[f32::NAN]]),
        encode(&[&[1.0], &[1.0, 2.0]]),
    ] {
        fs::write(&path, bytes).unwrap();
        assert!(bench::read_fvecs(&path, 10).is_err());
    }
    let bytes = encode(&[&[1.0, 2.0]]);
    for cut in 4..bytes.len() {
        fs::write(&path, &bytes[..cut]).unwrap();
        assert!(bench::read_fvecs(&path, 10).is_err());
    }
}

#[test]
fn file_benchmark_validates_query_dimensions_and_uses_exact_ground_truth() {
    let temp = TempDir::new();
    let base = temp.path().join("base.fvecs");
    let query = temp.path().join("query.fvecs");
    fs::write(&base, encode(&[&[1.0, 0.0], &[0.0, 1.0], &[-1.0, 0.0]])).unwrap();
    fs::write(&query, encode(&[&[0.9, 0.1], &[0.1, 0.9]])).unwrap();
    for metric in [Metric::SquaredL2, Metric::Cosine, Metric::InnerProduct] {
        let result = bench::run_fvecs(&base, &query, 10, 10, 128, metric).unwrap();
        assert_eq!(result.config.count, 3);
        assert_eq!(result.config.queries, 2);
        assert_eq!(result.recall_at_k, 1.0);
    }
    fs::write(&query, encode(&[&[1.0]])).unwrap();
    assert!(bench::run_fvecs(&base, &query, 10, 10, 128, Metric::SquaredL2).is_err());
}

#[test]
fn invalid_benchmark_resources_are_rejected_before_large_fixture_allocation() {
    assert!(
        bench::run(BenchConfig {
            count: 100000,
            dimensions: 4096,
            ..BenchConfig::default()
        })
        .is_err()
    );
    assert!(
        bench::run(BenchConfig {
            ef_search: 0,
            ..BenchConfig::default()
        })
        .is_err()
    );
    assert!(
        bench::run(BenchConfig {
            count: usize::MAX,
            ..BenchConfig::default()
        })
        .is_err()
    );
}
