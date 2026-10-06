//! Long-lived application backend with bounded asynchronous query submission.
#![forbid(unsafe_code)]
#[path = "support/app_backend.rs"]
mod app_backend;
use std::time::Duration;
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
    let mut callers = Vec::new();
    for _ in 0..4 {
        let client = client.clone();
        callers.push(std::thread::spawn(move || {
            let pending = client
                .submit(
                    &client.space().model,
                    vec![1.0, 0.1, 0.0],
                    5,
                    None,
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
    drop(client);
    let mut collection = worker.join().map_err(|_| "query worker panicked")?;
    collection.checkpoint()?;
    println!("Four application clients completed; collection closed cleanly.");
    Ok(())
}
