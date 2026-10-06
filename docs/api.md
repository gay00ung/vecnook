# Embed Vecnook in a Rust application

Start with `Database` when vectors need to survive a restart. Use `VectorIndex` for an in-memory index that your application rebuilds from another source. Both expose original vectors and opaque UTF-8 metadata, exact and approximate search, and the same fixed `Config`.

Build the complete, checked API reference locally:

```bash
cargo doc --offline --no-deps --open
```

Public items are required to have rustdoc. CI treats missing documentation and documentation warnings as errors, and runs the Rust examples embedded in the docs.

## Inputs and result ordering

Choose dimensions, metric, M and efConstruction before constructing the database. Config defaults are M=16, efConstruction=200, seed=42 and squared L2. `Config::new` does not validate by itself; `create`, `VectorIndex::new` and `Config::validate` do.

Coordinates are finite `f32`. Dimension mismatch, NaN/infinity, and a zero vector for cosine return `Error::InvalidInput`. IDs cover the full `u64` range. Metadata is UTF-8 with a 16 KiB bound; it is application data, never executed. The README lists all storage and transaction limits.

Results sort by ascending distance then ID. K=0 is empty and K larger than the eligible set is capped. `SearchStrategy::Hnsw` requires efSearch≥min(K, eligible count) and efSearch in 1..=4096. Exact strategy ignores efSearch. Auto uses exact search for small sets or oversized K and repairs an underfilled graph result. `complete` describes candidate count, not recall.

Use `search_metadata(query, k, options, value)` for indexed equality of a complete metadata string, or `search_ids(query, k, options, ids)` for an application-selected subset. Unfiltered `search` prepares eligibility without a full record predicate scan. Arbitrary `search_filtered` still evaluates its predicate once per active record. The report's `filter_evaluations` counts these predicate/equality comparisons separately from vector distance evaluations.

```rust
use vecnook::{Config, Metric, SearchOptions, VectorIndex};

let mut index = VectorIndex::new(Config::new(2).with_metric(Metric::Cosine))?;
index.put(7, &[1.0, 0.0], "project-a")?;
let result = index.search(&[0.9, 0.1], 10, SearchOptions::default())?;
assert_eq!(result.neighbors[0].id, 7);
# Ok::<(), vecnook::Error>(())
```

## Writes, ownership and errors

`Database::create` creates missing directories and refuses existing database files. `open` validates the snapshot and complete WAL frames. A handle keeps an exclusive OS lock until dropped; another process or handle cannot open that directory even for reading. Put the one application handle inside an `RwLock` for concurrent read access and exclusive mutation.

`put` returns true for a newly active ID and false for replacement. `delete` returns false for an absent ID without writing a WAL frame. Replacement and deletion retain old physical nodes until compaction. `write_batch` prevalidates all ordered operations and syncs one frame before modifying memory. Operations on the same ID observe earlier operations in that batch.

| Error | Application response |
| --- | --- |
| `InvalidInput` | Correct input values; invalid writes do not modify storage |
| `Capacity` | Inspect resource/limit/required; compact or reduce the batch |
| `EmbeddingMismatch` | Use the stored model identity or rebuild a new collection |
| `Conflict` | Reread current state before retrying a conditional batch |
| `Locked` | Reuse the application's existing handle or wait for the owner to close |
| `AlreadyExists` | Open the existing database, or choose a new create/backup destination |
| `Io` during mutation/maintenance | Close and reopen, then inspect affected IDs; the operation may have persisted |
| `Poisoned` | Close and reopen before further writes |
| `Corrupt` | Preserve the files and investigate or restore a verified backup; authoritative corruption is not silently skipped |
| `UnsupportedPlatform` | Persistent storage is unavailable; an in-memory index can still be used |

There are no implicit retries for storage failures. Dropping the handle releases the lock but does not checkpoint. Acknowledged writes are already synchronized to the WAL. `checkpoint` limits future replay work, `compact` reclaims deleted nodes, and `maintain` applies explicit thresholds synchronously. `backup` creates a fresh independently openable copy and refuses an existing directory.

See [embedded.rs](../examples/embedded.rs) for batch, reopen and backup, and the [document search demo](../examples/documents/README.md) for a real embedding client. Keep source data, embeddings and backups outside public source control.

See [compatibility](compatibility.md) for the 0.4 constructor/JSON migration, and [operations](operations.md) for read-only diagnostics, capacity and logical transfer.
