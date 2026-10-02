use std::path::PathBuf;
use vecnook::{Config, Database, MaintenancePolicy, Metric, Mutation, Result, SearchOptions};

fn main() -> Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/embedded"));
    let backup = path.with_file_name(format!(
        "{}-backup",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    let mut db = Database::create(&path, Config::new(3).with_metric(Metric::Cosine))?;
    let written = db.write_batch(&[
        Mutation::Put {
            id: 1,
            vector: &[1.0, 0.0, 0.0],
            metadata: "tenant-a",
        },
        Mutation::Put {
            id: 2,
            vector: &[0.0, 1.0, 0.0],
            metadata: "tenant-b",
        },
    ])?;
    println!(
        "batch sequence={} inserted={}",
        written.sequence, written.inserted
    );
    let found = db.search_filtered(&[1.0, 0.1, 0.0], 10, SearchOptions::default(), |r| {
        r.metadata == "tenant-a"
    })?;
    assert_eq!(found.neighbors[0].id, 1);
    println!(
        "mode={:?} reason={:?} neighbors={:?}",
        found.mode, found.reason, found.neighbors
    );
    db.checkpoint()?;
    drop(db);
    let mut db = Database::open(&path)?;
    assert!(db.recovery_info().graph_cache_loaded);
    println!("cached_nodes={}", db.recovery_info().cached_nodes);
    db.backup(&backup)?;
    let copy = Database::open(&backup)?;
    assert_eq!(copy.get(1), db.get(1));
    println!("backup={}", backup.display());
    println!(
        "maintenance={:?}",
        db.maintain(MaintenancePolicy::default())?.action
    );
    Ok(())
}
