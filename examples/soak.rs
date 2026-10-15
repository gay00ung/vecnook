//! Persistent mixed workload checked against an independently replayed acknowledgement log.
#![forbid(unsafe_code)]
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use vecnook::{
    Collection, Config, Database, Document, DocumentFilter, DocumentMutation, HybridSearchOptions,
    Mutation, SearchOptions, SearchReason, VectorIndex,
};
type Model = BTreeMap<u64, ([f32; 4], String)>;
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2));
    s.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
        .collect()
}
fn put_line(sequence: u64, id: u64, v: &[f32; 4], text: &str) -> String {
    format!(
        "{sequence}\tput\t{id}\t{}\t{}\n",
        v.iter()
            .map(|v| format!("{:08x}", v.to_bits()))
            .collect::<Vec<_>>()
            .join(","),
        hex(text.as_bytes())
    )
}
fn replay(path: &Path) -> Model {
    let mut model = Model::new();
    for line in fs::read_to_string(path).unwrap().lines() {
        let parts = line.split('\t').collect::<Vec<_>>();
        let id = parts[2].parse().unwrap();
        if parts[1] == "put" {
            let v = parts[3]
                .split(',')
                .map(|x| f32::from_bits(u32::from_str_radix(x, 16).unwrap()))
                .collect::<Vec<_>>();
            model.insert(
                id,
                (
                    v.try_into().unwrap(),
                    String::from_utf8(unhex(parts[4])).unwrap(),
                ),
            );
        } else {
            assert_eq!(parts[1], "delete");
            model.remove(&id);
        }
    }
    model
}
fn verify(db: &Database, model: &Model) {
    assert_eq!(db.len(), model.len());
    for (&id, (v, text)) in model {
        let record = db.get(id).unwrap();
        assert_eq!(&record.metadata, text);
        assert_eq!(record.vector.as_slice(), v);
    }
    let query = [0.1f32, 0.2, 0.3, 0.4];
    let mut oracle = model
        .iter()
        .map(|(&id, (v, _))| {
            let d = v
                .iter()
                .zip(query)
                .map(|(&a, b)| (f64::from(a) - f64::from(b)).powi(2))
                .sum::<f64>();
            (id, d)
        })
        .collect::<Vec<_>>();
    oracle.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    let found = db.search_exact(&query, 10).unwrap();
    assert_eq!(
        found.neighbors.iter().map(|n| n.id).collect::<Vec<_>>(),
        oracle.iter().take(10).map(|x| x.0).collect::<Vec<_>>()
    );
    db.check_invariants().unwrap();
}
fn document(id: u64, text: &str) -> Document {
    let mut document = Document::new(id, text, format!("source-{}.md", id % 7));
    document.tags = vec!["all".into(), format!("bucket-{}", id % 3)];
    document
}
fn verify_documents(collection: &Collection, model: &Model) {
    assert_eq!(collection.len(), model.len());
    for (&id, (vector, text)) in model {
        assert_eq!(collection.get(id).unwrap(), Some(document(id, text)));
        assert_eq!(collection.vector(id).unwrap(), vector);
    }
    let filter = DocumentFilter::default().with_tags(&["bucket-0"]);
    let keywords = collection
        .search_keywords("errno_3", model.len(), filter)
        .unwrap();
    let expected = model
        .keys()
        .copied()
        .filter(|id| id % 3 == 0 && id % 17 == 3)
        .collect::<Vec<_>>();
    assert_eq!(
        keywords
            .neighbors
            .iter()
            .map(|n| n.document.id)
            .collect::<Vec<_>>(),
        expected
    );
    assert!(
        collection
            .search_keywords("never_present", 10, filter)
            .unwrap()
            .neighbors
            .is_empty()
    );
    let vector_query = [0.1, 0.2, 0.3, 0.4];
    let options = HybridSearchOptions::default();
    let hybrid = collection
        .search_hybrid(&vector_query, "errno_3", 10, options, filter)
        .unwrap();
    assert!(hybrid.complete);
    let vector = collection
        .search(&vector_query, 64, options.vector, filter)
        .unwrap();
    let lexical = collection.search_keywords("errno_3", 64, filter).unwrap();
    // Independently compute the union/ranks and fused order from the two branches.
    let mut oracle = BTreeMap::<u64, f64>::new();
    for (rank, neighbor) in vector.neighbors.iter().enumerate() {
        *oracle.entry(neighbor.document.id).or_default() += 1.0 / (61.0 + rank as f64);
    }
    for (rank, neighbor) in lexical.neighbors.iter().enumerate() {
        *oracle.entry(neighbor.document.id).or_default() += 1.0 / (61.0 + rank as f64);
    }
    let mut oracle = oracle.into_iter().collect::<Vec<_>>();
    oracle.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    assert_eq!(
        hybrid
            .neighbors
            .iter()
            .map(|n| (n.document.id, n.score))
            .collect::<Vec<_>>(),
        oracle.into_iter().take(10).collect::<Vec<_>>()
    );
    collection.check_invariants().unwrap();
}
fn verify_policy(index: &VectorIndex) {
    let report = index
        .search_metadata(&[17.2], 10, SearchOptions::default(), "selected")
        .unwrap();
    assert_eq!(report.reason, SearchReason::SelectiveFilter);
    assert_eq!(report.distance_computations, 500);
    let mut ids = (0..500u64).collect::<Vec<_>>();
    ids.sort_by(|&a, &b| {
        ((a as f64 - 17.2f32 as f64).powi(2))
            .total_cmp(&(b as f64 - 17.2f32 as f64).powi(2))
            .then(a.cmp(&b))
    });
    assert_eq!(
        report.neighbors.iter().map(|n| n.id).collect::<Vec<_>>(),
        ids[..10]
    );
}
fn progress(
    root: &Path,
    start: Instant,
    mutations: u64,
    restarts: u64,
    backups: u64,
    complete: bool,
) -> std::io::Result<()> {
    let elapsed = start.elapsed().as_secs_f64();
    let text = format!(
        "{{\"workload\":\"vectors+bm25+rrf+selective-auto\",\"completed\":{complete},\"elapsed_seconds\":{elapsed},\"ack_mutations\":{mutations},\"restarts\":{restarts},\"backups_verified\":{backups},\"duration_gate_24h\":{},\"mutation_gate_100k\":{}}}\n",
        elapsed >= 86400.0,
        mutations >= 100000
    );
    fs::write(root.join("progress.json"), &text)?;
    if complete {
        print!("{text}");
    }
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() || args.len() > 3 {
        return Err(
            "usage: soak <new-root> [minimum-seconds=60] [minimum-mutations=100000]".into(),
        );
    }
    let root = PathBuf::from(&args[0]);
    let seconds = args.get(1).map_or(Ok(60u64), |s| s.parse())?;
    let target = args.get(2).map_or(Ok(100000u64), |s| s.parse())?;
    if target == 0 || seconds > 7 * 86400 {
        return Err("mutations must be positive; duration must be at most seven days".into());
    }
    if let Some(parent) = root.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(&root)?;
    let start = Instant::now();
    let ack_path = root.join("ack.tsv");
    let mut ack = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&ack_path)?;
    let db_path = root.join("database");
    let mut db = Database::create(&db_path, Config::new(4))?;
    let mut collection = Collection::create(&root, "documents", Config::new(4), "soak-fixture-v1")?;
    let space = collection.space().clone();
    let mut policy = VectorIndex::new(Config::new(1))?;
    for id in 0..4000 {
        policy.put(
            id,
            &[id as f32],
            if id < 500 { "selected" } else { "other" },
        )?;
    }
    verify_policy(&policy);
    let mut model = Model::new();
    let mut seed = 42u64;
    let mut iterations = 0u64;
    let mut mutations = 0u64;
    let mut restarts = 0;
    let mut backups = 0;
    let mut last_progress = Instant::now();
    progress(&root, start, 0, 0, 0, false)?;
    while mutations < target || start.elapsed() < Duration::from_secs(seconds) {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let id = seed % 257;
        let value = (seed >> 32) as u32 as f32 / u32::MAX as f32;
        let vector = [
            value,
            1.0 - value,
            (id % 13) as f32,
            iterations as f32 / 100000.0,
        ];
        let text = format!(
            "original 한글 step {iterations} lookup_{id} errno_{}\n",
            id % 17
        );
        let chunk = document(id, &text);
        let mut log = String::new();
        match iterations % 5 {
            0 => {
                let deleted = db.delete(id)?;
                assert_eq!(collection.delete(id)?, deleted);
                if deleted {
                    model.remove(&id);
                    log = format!("{}\tdelete\t{id}\n", db.sequence());
                    mutations += 1;
                }
            }
            1 => {
                let other = (id + 1) % 257;
                let report = db.write_batch(&[
                    Mutation::Put {
                        id,
                        vector: &vector,
                        metadata: &text,
                    },
                    Mutation::Delete { id: other },
                ])?;
                collection.write_batch(&[
                    DocumentMutation::Put {
                        document: &chunk,
                        vector: &vector,
                    },
                    DocumentMutation::Delete { id: other },
                ])?;
                model.insert(id, (vector, text.clone()));
                log = put_line(db.sequence(), id, &vector, &text);
                if model.remove(&other).is_some() {
                    log.push_str(&format!("{}\tdelete\t{other}\n", db.sequence()));
                }
                mutations += (report.inserted + report.updated + report.deleted) as u64;
            }
            _ => {
                db.put(id, &vector, &text)?;
                collection.put(&chunk, &vector)?;
                model.insert(id, (vector, text.clone()));
                log = put_line(db.sequence(), id, &vector, &text);
                mutations += 1;
            }
        }
        if !log.is_empty() {
            ack.write_all(log.as_bytes())?;
            ack.sync_all()?;
        }
        iterations += 1;
        if iterations.is_multiple_of(500) {
            db.compact()?;
            collection.compact()?;
            verify(&db, &model);
            verify_documents(&collection, &model);
            verify_policy(&policy);
        }
        if iterations.is_multiple_of(2000) {
            drop(db);
            drop(collection);
            db = Database::open(&db_path)?;
            collection = Collection::open(&root, "documents", &space)?;
            assert_eq!(replay(&ack_path), model);
            verify(&db, &model);
            verify_documents(&collection, &model);
            restarts += 1;
            progress(&root, start, mutations, restarts, backups, false)?;
        }
        if iterations.is_multiple_of(10000) {
            let path = root.join(format!("backup-{backups}"));
            db.backup(&path)?;
            let copy = Database::open(&path)?;
            verify(&copy, &model);
            drop(copy);
            fs::remove_dir_all(path)?;
            let name = format!("doc-backup-{backups}");
            collection.backup(&root, &name)?;
            let copy = Collection::open(&root, &name, &space)?;
            verify_documents(&copy, &model);
            drop(copy);
            fs::remove_dir_all(root.join(name))?;
            let export = root.join("doc-export.bin");
            collection.export(&export)?;
            let restored = Collection::import(&export, &root, "doc-restored")?;
            verify_documents(&restored, &model);
            drop(restored);
            fs::remove_dir_all(root.join("doc-restored"))?;
            fs::remove_file(export)?;
            backups += 1;
        }
        if last_progress.elapsed() >= Duration::from_secs(60) {
            progress(&root, start, mutations, restarts, backups, false)?;
            last_progress = Instant::now();
        }
        if mutations >= target {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    ack.sync_all()?;
    drop(ack);
    verify(&db, &model);
    verify_documents(&collection, &model);
    assert_eq!(replay(&ack_path), model);
    db.checkpoint()?;
    collection.checkpoint()?;
    drop(db);
    drop(collection);
    let db = Database::open(&db_path)?;
    verify(&db, &model);
    let collection = Collection::open(&root, "documents", &space)?;
    verify_documents(&collection, &model);
    verify_policy(&policy);
    drop(db);
    progress(&root, start, mutations, restarts + 1, backups, true)?;
    Ok(())
}
