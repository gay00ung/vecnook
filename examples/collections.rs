//! Persist original chunks and isolate projects/embedding models in named collections.
use vecnook::{
    Collection, Config, Document, DocumentFilter, EmbeddingSpace, Metric, SearchOptions,
};

fn main() -> vecnook::Result<()> {
    let root = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "data/projects".into());
    let space = EmbeddingSpace::new("example-model-v1/retrieval-prompt-v1", 3, Metric::Cosine);
    let mut collection = Collection::create(
        &root,
        "notes",
        Config::new(3).with_metric(space.metric),
        &space.model,
    )?;
    let mut doc = Document::new(
        7,
        "Acknowledged changes survive a process restart.",
        "recovery.md",
    );
    doc.tags = vec!["rust".into(), "storage".into()];
    doc.start_line = 4;
    doc.end_line = 6;
    collection.put(&doc, &[1.0, 0.0, 0.0])?;
    collection.checkpoint()?;
    drop(collection);
    let collection = Collection::open(&root, "notes", &space)?;
    let result = collection.search(
        &[0.9, 0.1, 0.0],
        5,
        SearchOptions::default(),
        DocumentFilter::default()
            .with_optional_source(None)
            .with_tags(&["storage"]),
    )?;
    assert_eq!(result.neighbors[0].document, doc);
    println!(
        "{}:{} {}",
        doc.source, doc.start_line, result.neighbors[0].document.text
    );
    Ok(())
}
