mod support;

use std::collections::BTreeMap;
use support::TempDir;
use vecnook::{Config, Database, Metric, Mutation, SearchOptions};

type Model = BTreeMap<u64, (Vec<f32>, String)>;

fn next(state: &mut u64) -> u64 {
    *state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
    *state >> 16
}

fn vector(state: &mut u64) -> Vec<f32> {
    vec![
        (next(state) % 997) as f32 / 7.0 - 60.0,
        (next(state) % 991) as f32 / 11.0 - 40.0,
        1.0,
    ]
}

fn verify(db: &Database, model: &Model, metric: Metric, query: &[f32]) {
    db.check_invariants().unwrap();
    assert_eq!(db.stats().active_records, model.len());
    for id in 0..32 {
        match (db.get(id), model.get(&id)) {
            (Some(actual), Some((vector, metadata))) => {
                assert_eq!(&actual.vector, vector);
                assert_eq!(&actual.metadata, metadata);
            }
            (None, None) => {}
            _ => panic!("model disagreement for ID {id}"),
        }
    }
    let mut expected: Vec<_> = model
        .iter()
        .map(|(&id, (vector, _))| {
            let dot: f64 = query
                .iter()
                .zip(vector)
                .map(|(&q, &v)| f64::from(q) * f64::from(v))
                .sum();
            let distance = match metric {
                Metric::SquaredL2 => query
                    .iter()
                    .zip(vector)
                    .map(|(&q, &v)| (f64::from(q) - f64::from(v)).powi(2))
                    .sum(),
                Metric::InnerProduct => -dot,
                Metric::Cosine => {
                    let norm = |values: &[f32]| {
                        values
                            .iter()
                            .map(|&v| f64::from(v).powi(2))
                            .sum::<f64>()
                            .sqrt()
                    };
                    (1.0 - dot / (norm(query) * norm(vector))).clamp(0.0, 2.0)
                }
            };
            (distance, id)
        })
        .collect();
    expected.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let found = db.search_exact(query, 32).unwrap();
    assert!(found.complete);
    assert_eq!(found.neighbors.len(), expected.len());
    for (actual, &(distance, id)) in found.neighbors.iter().zip(&expected) {
        assert_eq!(actual.id, id);
        assert!((actual.distance - distance).abs() <= 1e-10 * (1.0 + distance.abs()));
    }
    let filtered = db
        .search_filtered(query, 5, SearchOptions::default(), |r| {
            r.metadata == "tenant-b"
        })
        .unwrap();
    let expected: Vec<_> = expected
        .into_iter()
        .filter(|(_, id)| model[id].1 == "tenant-b")
        .map(|(_, id)| id)
        .collect();
    assert_eq!(filtered.eligible_count, expected.len());
    assert!(filtered.complete);
    assert_eq!(
        filtered.neighbors.iter().map(|r| r.id).collect::<Vec<_>>(),
        expected.into_iter().take(5).collect::<Vec<_>>()
    );
}

fn exercise(metric: Metric) {
    let temp = TempDir::new();
    let mut db = Database::create(temp.path(), Config::new(3).with_metric(metric)).unwrap();
    let mut model = Model::new();
    let mut state = 42;
    let mut sequence = 0;
    for step in 0..300 {
        let id = next(&mut state) % 32;
        match next(&mut state) % 7 {
            0..=2 => {
                let vector = vector(&mut state);
                let metadata = if id.is_multiple_of(3) {
                    "tenant-b"
                } else {
                    "tenant-a"
                };
                let inserted = model
                    .insert(id, (vector.clone(), metadata.into()))
                    .is_none();
                assert_eq!(db.put(id, &vector, metadata).unwrap(), inserted);
                sequence += 1;
            }
            3 => {
                let existed = model.remove(&id).is_some();
                assert_eq!(db.delete(id).unwrap(), existed);
                sequence += u64::from(existed);
            }
            4 => {
                // Same-ID operations deliberately occur in one ordered transaction.
                let rows: Vec<_> = (0..4)
                    .map(|i| {
                        let member_id = if i < 2 { id } else { next(&mut state) % 32 };
                        let value = if next(&mut state).is_multiple_of(3) {
                            None
                        } else {
                            Some(vector(&mut state))
                        };
                        (member_id, value)
                    })
                    .collect();
                let mut changed = false;
                for (id, vector) in &rows {
                    if let Some(vector) = vector {
                        model.insert(*id, (vector.clone(), "tenant-b".into()));
                        changed = true;
                    } else {
                        changed |= model.remove(id).is_some();
                    }
                }
                let operations: Vec<_> = rows
                    .iter()
                    .map(|(id, vector)| match vector {
                        Some(vector) => Mutation::Put {
                            id: *id,
                            vector,
                            metadata: "tenant-b",
                        },
                        None => Mutation::Delete { id: *id },
                    })
                    .collect();
                let report = db.write_batch(&operations).unwrap();
                sequence += u64::from(changed);
                assert_eq!(report.sequence, sequence);
            }
            5 => {
                if step % 2 == 0 {
                    db.checkpoint().unwrap();
                } else {
                    let before = db.stats().physical_nodes;
                    assert_eq!(db.compact().unwrap(), before - model.len());
                    assert_eq!(db.stats().physical_nodes, model.len());
                }
            }
            _ => {
                drop(db);
                db = Database::open(temp.path()).unwrap();
            }
        }
        assert_eq!(db.sequence(), sequence);
        verify(&db, &model, metric, &vector(&mut state));
    }
    db.checkpoint().unwrap();
    drop(db);
    let db = Database::open(temp.path()).unwrap();
    assert!(db.recovery_info().graph_cache_loaded);
    assert_eq!(db.sequence(), sequence);
    verify(&db, &model, metric, &[2.3, -1.7, 1.0]);
}

#[test]
fn l2_crud_batches_maintenance_and_restarts_match_model() {
    exercise(Metric::SquaredL2);
}

#[test]
fn cosine_crud_batches_maintenance_and_restarts_match_model() {
    exercise(Metric::Cosine);
}

#[test]
fn inner_product_crud_batches_maintenance_and_restarts_match_model() {
    exercise(Metric::InnerProduct);
}
