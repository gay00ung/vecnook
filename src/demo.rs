use crate::demo_data::{DIMENSIONS, DOCUMENTS, MODEL, QUERIES};
use std::{path::PathBuf, time::Instant};
use vecnook::{
    Collection, Config, Document, DocumentFilter, EmbeddingSpace, Error, Metric, Result,
    SearchOptions, SearchStrategy,
};

fn document(i: usize, source: &str, text: &str, end: u32) -> Document {
    let mut doc = Document::new(i as u64, text, source);
    doc.end_line = end;
    doc.tags = vec!["demo".into()];
    doc
}

pub fn run(args: &[String]) -> Result<()> {
    if args.len() > 2 {
        return Err(Error::InvalidInput(
            "demo [new-or-existing-demo-root] [query-number=1..3]; prepared queries only".into(),
        ));
    }
    let selected = args
        .get(1)
        .map(|x| x.parse::<usize>())
        .transpose()
        .map_err(|_| Error::InvalidInput("choose a prepared query number: 1, 2 or 3".into()))?;
    if selected.is_some_and(|n| !(1..=QUERIES.len()).contains(&n)) {
        return Err(Error::InvalidInput(
            "choose a prepared query number: 1, 2 or 3".into(),
        ));
    }
    let root = args.first().map_or_else(
        || std::env::temp_dir().join(format!("vecnook-demo-{}", std::process::id())),
        PathBuf::from,
    );
    let space = EmbeddingSpace::new(MODEL, DIMENSIONS, Metric::Cosine);
    let started = Instant::now();
    let mut collection = if root.join("demo").exists() {
        let c = Collection::open(&root, "demo", &space)?;
        if c.len() != DOCUMENTS.len() {
            return Err(Error::InvalidInput(
                "existing demo has changed; choose a fresh root".into(),
            ));
        }
        for (i, (source, text, end, vector)) in DOCUMENTS.iter().enumerate() {
            let doc = c
                .get(i as u64)?
                .ok_or_else(|| Error::Corrupt("missing demo document".into()))?;
            if doc != document(i, source, text, *end) || c.vector(i as u64) != Some(*vector) {
                return Err(Error::InvalidInput(
                    "existing demo has changed; choose a fresh root".into(),
                ));
            }
        }
        println!(
            "Reopened saved demo: graph_cache_loaded={}",
            c.recovery_info().graph_cache_loaded
        );
        c
    } else {
        if root.exists() {
            return Err(Error::AlreadyExists);
        }
        let mut c = Collection::create(
            &root,
            "demo",
            Config::new(DIMENSIONS).with_metric(Metric::Cosine),
            MODEL,
        )?;
        for (i, (source, text, end, vector)) in DOCUMENTS.iter().enumerate() {
            let doc = document(i, source, text, *end);
            c.put(&doc, vector)?;
        }
        c.checkpoint()?;
        c
    };
    println!(
        "Vecnook offline demo: {} original documents, {}-dimensional real embeddings",
        collection.len(),
        DIMENSIONS
    );
    println!("Prepared queries only; no network or model installation is used at runtime.");
    let options = SearchOptions::default().with_strategy(SearchStrategy::Hnsw);
    for (i, (query, vector)) in QUERIES.iter().enumerate() {
        if selected.is_some_and(|n| n != i + 1) {
            continue;
        }
        let result = collection.search(vector, 1, options, DocumentFilter::default())?;
        println!("\nQuery {}: {query} (mode={:?})", i + 1, result.search.mode);
        for n in result.neighbors {
            println!(
                "{}:{}-{} cosine_distance={:.6}\n{}",
                n.document.source,
                n.document.start_line,
                n.document.end_line,
                n.distance,
                n.document.text
            );
        }
    }
    collection.checkpoint()?;
    println!(
        "Saved under {} (ready in {:.3}s). Re-run: vecnook demo {:?}",
        root.display(),
        started.elapsed().as_secs_f64(),
        root
    );
    println!(
        "For your own documents and free-text queries, see examples/documents/README.md (local Ollama model required)."
    );
    Ok(())
}
