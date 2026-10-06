#[path = "../examples/support/app_backend.rs"]
mod backend;
mod support;
use std::time::Duration;
use support::TempDir;
use vecnook::{Collection, Config, Document, EmbeddingSpace, Error, Metric};

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
