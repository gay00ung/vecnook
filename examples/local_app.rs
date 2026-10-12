//! Long-lived application backend with ordered writes, bounded queries and restart.
#![forbid(unsafe_code)]
use std::time::Duration;
use vecnook::app::{self as app_backend, WriteOperation};
use vecnook::{Collection, Config, Document, EmbeddingSpace, Metric};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "data/local-app".into());
    let space = EmbeddingSpace::new("fixture-vectors-v1", 3, Metric::Cosine);
    let collection = if std::path::Path::new(&root).join("notes").exists() {
        Collection::open(&root, "notes", &space)?
    } else {
        let mut collection = Collection::create(
            &root,
            "notes",
            Config::new(3).with_metric(space.metric),
            &space.model,
        )?;
        let mut doc = Document::new(1, "Acknowledged changes survive a restart.", "recovery.md");
        doc.tags = vec!["rust".into()];
        collection.put(&doc, &[1.0, 0.0, 0.0])?;
        collection.checkpoint()?;
        collection
    };
    let (client, worker) = app_backend::start(collection)?;
    let mut updated = Document::new(
        1,
        "Live updates and acknowledged changes survive a restart.",
        "recovery.md",
    );
    updated.tags = vec!["rust".into()];
    let pending_write = client.submit_batch(
        &space.model,
        vec![
            WriteOperation::Put {
                document: updated.clone(),
                vector: vec![1.0, 0.0, 0.0],
            },
            WriteOperation::Put {
                document: Document::new(2, "Temporary note", "temporary.md"),
                vector: vec![0.0, 1.0, 0.0],
            },
        ],
        None,
    )?;
    println!("Write queued; application remains free to handle input.");
    pending_write.recv_timeout(Duration::from_secs(10))??;
    let mut callers = Vec::new();
    for _ in 0..4 {
        let client = client.clone();
        callers.push(std::thread::spawn(move || {
            let pending = client
                .submit(
                    &client.space().model,
                    vec![1.0, 0.1, 0.0],
                    5,
                    Some("recovery.md".into()),
                    vec!["rust".into()],
                )
                .unwrap();
            let report = pending
                .recv_timeout(Duration::from_secs(10))
                .unwrap()
                .unwrap();
            assert_eq!(report.neighbors[0].document.id, 1);
            report.neighbors[0].document.text.clone()
        }));
    }
    for caller in callers {
        println!("{}", caller.join().unwrap());
    }
    let deletion =
        client.submit_batch(&space.model, vec![WriteOperation::Delete { id: 2 }], None)?;
    // Closing a response receiver does not cancel an accepted mutation.
    drop(deletion);
    client
        .checkpoint()?
        .recv_timeout(Duration::from_secs(10))??;
    drop(client);
    let mut collection = worker.join().map_err(|_| "query worker panicked")?;
    assert_eq!(collection.get(1)?, Some(updated.clone()));
    assert!(collection.get(2)?.is_none());
    collection.checkpoint()?;
    drop(collection);
    let reopened = Collection::open(&root, "notes", &space)?;
    assert_eq!(reopened.get(1)?, Some(updated));
    assert!(reopened.get(2)?.is_none());
    println!("Four application clients completed; collection closed cleanly.");
    Ok(())
}
