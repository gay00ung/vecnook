#[path = "../examples/support/app_backend.rs"]
mod backend;
mod support;
use std::time::Duration;
use support::TempDir;
use vecnook::{Collection, Config, Document, EmbeddingSpace, Error, Metric};

#[test]
fn ordered_live_batches_are_atomic_and_abandoned_writes_drain_before_restart() {
    use vecnook::app::WriteOperation;
    let temp = TempDir::new();
    let schema = space();
    let collection = Collection::create(
        temp.path(),
        "app",
        Config::new(2).with_metric(schema.metric),
        &schema.model,
    )
    .unwrap();
    let (client, worker) = backend::start(collection).unwrap();
    let doc = Document::new(u64::MAX, "updated original\r\n한글", "notes.md");
    let accepted = client
        .submit_batch(
            "fixture-v1",
            vec![WriteOperation::Put {
                document: doc.clone(),
                vector: vec![1.0, 0.0],
            }],
            Some(0),
        )
        .unwrap();
    let after_write = client
        .submit("fixture-v1", vec![1.0, 0.0], 1, None, vec![])
        .unwrap();
    accepted
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .unwrap();
    assert_eq!(
        after_write
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .unwrap()
            .neighbors[0]
            .document,
        doc
    );
    let conflict = client
        .submit_batch(
            "fixture-v1",
            vec![WriteOperation::Delete { id: u64::MAX }],
            Some(0),
        )
        .unwrap();
    assert!(matches!(
        conflict.recv_timeout(Duration::from_secs(10)).unwrap(),
        Err(Error::Conflict { .. })
    ));
    // A valid delete followed by invalid coordinates must not partially delete the old document.
    let invalid = client
        .submit_batch(
            "fixture-v1",
            vec![
                WriteOperation::Delete { id: u64::MAX },
                WriteOperation::Put {
                    document: Document::new(2, "bad", "b.md"),
                    vector: vec![f32::NAN, 0.0],
                },
            ],
            None,
        )
        .unwrap();
    assert!(matches!(
        invalid.recv_timeout(Duration::from_secs(10)).unwrap(),
        Err(Error::InvalidInput(_))
    ));
    drop(
        client
            .submit_batch(
                "fixture-v1",
                vec![WriteOperation::Put {
                    document: Document::new(3, "drained", "c.md"),
                    vector: vec![0.0, 1.0],
                }],
                None,
            )
            .unwrap(),
    );
    let checkpoint = client.checkpoint().unwrap();
    drop(client);
    checkpoint
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .unwrap();
    let collection = worker.join().unwrap();
    assert_eq!(collection.sequence(), 2);
    assert_eq!(collection.get(u64::MAX).unwrap(), Some(doc.clone()));
    assert!(collection.get(2).unwrap().is_none());
    assert_eq!(collection.get(3).unwrap().unwrap().text, "drained");
    drop(collection);
    let reopened = Collection::open(temp.path(), "app", &schema).unwrap();
    assert_eq!(reopened.get(u64::MAX).unwrap(), Some(doc));
    assert_eq!(reopened.get(3).unwrap().unwrap().text, "drained");
}

#[test]
fn oversized_app_write_requests_are_rejected_before_enqueueing() {
    use vecnook::app::{SubmitError, WriteOperation};
    let temp = TempDir::new();
    let schema = space();
    let collection = Collection::create(
        temp.path(),
        "app",
        Config::new(2).with_metric(schema.metric),
        &schema.model,
    )
    .unwrap();
    let (client, worker) = backend::start(collection).unwrap();
    let puts = || {
        (0..64)
            .map(|id| WriteOperation::Put {
                document: Document::new(id, "x".repeat(6000), "a.md"),
                vector: vec![1.0, 0.0],
            })
            .collect()
    };
    assert!(matches!(
        client.submit_batch("fixture-v1", puts(), None),
        Err(SubmitError::InvalidRequest)
    ));
    assert!(matches!(
        client.submit_batch("other", vec![WriteOperation::Delete { id: 1 }], None),
        Err(SubmitError::InvalidRequest)
    ));
    assert!(matches!(
        client.submit_batch("fixture-v1", vec![], None),
        Err(SubmitError::InvalidRequest)
    ));
    assert!(matches!(
        client.submit_batch(
            "fixture-v1",
            vec![WriteOperation::Delete { id: 1 }; 65],
            None
        ),
        Err(SubmitError::InvalidRequest)
    ));
    drop(client);
    let collection = worker.join().unwrap();
    assert_eq!(collection.sequence(), 0);
}

fn space() -> EmbeddingSpace {
    EmbeddingSpace::new("fixture-v1", 2, Metric::Cosine)
}

#[test]
fn concurrent_application_clients_keep_one_lock_and_return_original_sources() {
    let temp = TempDir::new();
    let schema = space();
    let mut collection = Collection::create(
        temp.path(),
        "app",
        Config::new(2).with_metric(schema.metric),
        &schema.model,
    )
    .unwrap();
    let mut doc = Document::new(u64::MAX, "Original application text", "notes.md");
    doc.tags = vec!["rust".into()];
    collection.put(&doc, &[1.0, 0.0]).unwrap();
    let (client, worker) = backend::start(collection).unwrap();
    assert_eq!(client.space(), &schema);
    assert!(matches!(
        Collection::open(temp.path(), "app", &schema),
        Err(Error::Locked)
    ));
    let callers: Vec<_> = (0..8)
        .map(|_| {
            let client = client.clone();
            std::thread::spawn(move || {
                for _ in 0..20 {
                    let pending = client
                        .submit(
                            "fixture-v1",
                            vec![1.0, 0.0],
                            10,
                            Some("notes.md".into()),
                            vec!["rust".into()],
                        )
                        .unwrap();
                    let result = pending
                        .recv_timeout(Duration::from_secs(10))
                        .unwrap()
                        .unwrap();
                    assert_eq!(result.neighbors[0].document.id, u64::MAX);
                    assert_eq!(
                        result.neighbors[0].document.text,
                        "Original application text"
                    );
                }
            })
        })
        .collect();
    for caller in callers {
        caller.join().unwrap();
    }
    drop(client);
    let collection = worker.join().unwrap();
    collection.check_invariants().unwrap();
    drop(collection);
    let reopened = Collection::open(temp.path(), "app", &schema).unwrap();
    assert_eq!(reopened.get(u64::MAX).unwrap(), Some(doc));
}

#[test]
fn invalid_and_abandoned_requests_do_not_stop_the_worker_and_shutdown_drains_pending_work() {
    let temp = TempDir::new();
    let schema = space();
    let mut collection = Collection::create(
        temp.path(),
        "app",
        Config::new(2).with_metric(schema.metric),
        &schema.model,
    )
    .unwrap();
    collection
        .put(&Document::new(1, "text", "a.md"), &[1.0, 0.0])
        .unwrap();
    let (client, worker) = backend::start(collection).unwrap();
    assert!(matches!(
        client.submit("other-model", vec![1.0, 0.0], 1, None, vec![]),
        Err(backend::SubmitError::InvalidRequest)
    ));
    assert!(
        client
            .submit("fixture-v1", vec![1.0], 1, None, vec![])
            .is_err()
    );
    assert!(
        client
            .submit("fixture-v1", vec![1.0, 0.0], 101, None, vec![])
            .is_err()
    );
    let invalid = client
        .submit("fixture-v1", vec![f32::NAN, 0.0], 1, None, vec![])
        .unwrap();
    assert!(matches!(
        invalid.recv_timeout(Duration::from_secs(10)).unwrap(),
        Err(Error::InvalidInput(_))
    ));
    drop(
        client
            .submit("fixture-v1", vec![1.0, 0.0], 1, None, vec![])
            .unwrap(),
    );
    let pending = client
        .submit("fixture-v1", vec![1.0, 0.0], 1, None, vec![])
        .unwrap();
    drop(client);
    assert_eq!(
        pending
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .unwrap()
            .neighbors[0]
            .document
            .id,
        1
    );
    let collection = worker.join().unwrap();
    assert_eq!(collection.len(), 1);
}
