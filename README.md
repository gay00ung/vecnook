# Vecnook

A small embedded vector database for Rust applications, built with the standard library. Vecnook implements its own HNSW graph, exact search, and persistent storage. It has no external crates or vector search library dependencies.

[![CI](https://github.com/gay00ung/vecnook/actions/workflows/ci.yml/badge.svg)](https://github.com/gay00ung/vecnook/actions/workflows/ci.yml)
[crates.io](https://crates.io/crates/vecnook) · [API reference](https://docs.rs/vecnook/1.0.0-rc.1/vecnook/) · [MIT license](LICENSE) · [Changelog](CHANGELOG.md) · [Measured benchmarks](docs/benchmarks.md)

**1.0.0-rc.1 is a release candidate.** It targets local applications with one database handle, bounded datasets, and application-provided embeddings. Persistence supports macOS, Linux and Windows. The index stays in RAM. Stable publication requires the additional [candidate validation](docs/release-readiness.md), including a full 24-hour workload and Windows 11 NTFS verification.

## Get started

Install the candidate and run an [offline prepared-query demo](docs/quickstart.md) with original Markdown, source references and checkpoint/restart behavior:

Requires Rust 1.89 or later. The package, library, and executable are all named `vecnook`.

```bash
cargo install vecnook --version '=1.0.0-rc.1' --locked
vecnook demo data/first-demo
vecnook demo data/first-demo 1
```

For a Rust application, add the registry dependency:

```bash
cargo add vecnook@=1.0.0-rc.1
```

Or edit `Cargo.toml`:

```toml
[dependencies]
vecnook = "=1.0.0-rc.1"
```

The explicit prerelease version pins this candidate; commit your application lockfile as well. The [API reference](https://docs.rs/vecnook/1.0.0-rc.1/vecnook/) documents exported types and methods. See the [API guide](docs/api.md), [compatibility contracts](docs/compatibility.md) and [release procedure](docs/releasing.md). Versioned source is also available through the Git tag:

```bash
cargo install --git https://github.com/gay00ung/vecnook --tag v1.0.0-rc.1 --locked
```

To build a checkout:

```bash
cargo build --offline --release
cargo run --offline --example basic
cargo run --offline --example embedded -- data/embedded-demo
```

The embedded example creates a new database and backup. Choose a fresh path on each run.

## Embed a database

`VectorIndex` is an in-memory index. `Database` adds synced writes, locking, checkpoints, backups, and recovery.

```rust
use vecnook::{Config, Database, Metric, Mutation, Result, SearchOptions};

fn main() -> Result<()> {
    let mut db = Database::create("data/app", Config::new(3).with_metric(Metric::Cosine))?;
    db.write_batch(&[
        Mutation::Put { id: 1, vector: &[1.0, 0.0, 0.0], metadata: "tenant-a" },
        Mutation::Put { id: 2, vector: &[0.0, 1.0, 0.0], metadata: "tenant-b" },
    ])?;

    let result = db.search_filtered(&[1.0, 0.1, 0.0], 10, SearchOptions::default(), |r| {
        r.metadata == "tenant-a"
    })?;
    assert_eq!(result.eligible_count, 1);
    assert_eq!(result.neighbors[0].id, 1);
    println!("mode={:?} reason={:?}", result.mode, result.reason);

    db.checkpoint()?;
    drop(db);
    let db = Database::open("data/app")?;
    assert!(db.recovery_info().graph_cache_loaded);
    Ok(())
}
```

`create` refuses to overwrite a database; use `open` for an existing one. Reads use `&self` and mutations use `&mut self`. Share a persistent handle between threads with application synchronization such as `RwLock`. An immutable index supports concurrent readers. An exclusive OS file lock prevents a second handle or process from opening the same database directory.

For an event-loop or GUI application, the [`local_app` example](examples/local_app.rs) moves one collection onto a worker with a bounded 32-request queue. Cloneable clients submit without waiting for search, receive typed documents and shut down without reopening the locked directory. Run it twice against the same path to exercise restart; see the [application integration guide](docs/app-integration.md).

## Search and filtering

Choose a metric when creating the database. It is stored on disk and cannot change in place. Original `f32` coordinates are preserved; distances and norms use `f64`.

| Metric | Returned distance, sorted ascending |
| --- | --- |
| `Metric::SquaredL2` (default) | Sum of squared coordinate differences |
| `Metric::Cosine` | `1 - cosine_similarity`, clamped to [0, 2]; zero vectors are rejected |
| `Metric::InnerProduct` | Negative dot product; the highest dot product ranks first |

Neighbors sort by distance, then ID. Use `search_exact` as an exact reference and `search_hnsw` to explicitly request approximate search. M=16 and efConstruction=200 are the defaults. Tune efSearch against recall on your own queries; a larger efSearch usually examines more candidates.

`search` and `search_filtered` accept `SearchOptions`:

| Strategy | Behavior |
| --- | --- |
| `Exact` | Scan eligible vectors exactly |
| `Hnsw` | Search the graph; report an insufficient candidate count without switching modes |
| `Auto` (default) | Exact scan when eligible count ≤256 or K exceeds efSearch; otherwise HNSW, with exact repair if too few candidates are found |

`search_metadata` uses a maintained inverted index for exact equality of the complete metadata string. Only matching hash-bucket candidates are compared, with full string checks to reject collisions. `search_ids` accepts an application-selected subset and skips other records when preparing eligibility. Unfiltered `search` does not evaluate a predicate across all records. These indexes are rebuilt from authoritative records on open and updated through mutations and compaction.

`search_filtered` evaluates an arbitrary predicate once per active record, which costs O(active records). Ineligible/deleted nodes can be traversed but are not returned. Auto scans small eligible sets exactly. `filter_evaluations` reports predicate/metadata comparison counts; it does not count every graph operation. Graph traversal still initializes a visited bitmap proportional to physical node count. A predicate is not an access-control mechanism.

Results expose `mode`, `reason`, `eligible_count`, `distance_computations`, and `complete`. **`complete=true` means min(K, eligible count) results were returned; it does not certify recall.** Auto repairs a missing result count, not inaccurate full-length approximate results. K=0 returns no neighbors; K above the eligible count is capped at that count. For forced HNSW, efSearch must be at least the capped K.

## Writes, recovery, and maintenance

Putting an existing ID replaces its vector and metadata. Updates append a node and mark the old node deleted; deleting an absent ID is a no-op.

`write_batch` applies ordered Put/Delete operations in one synced WAL frame and advances the sequence once. It validates the entire input before writing. Operations on the same ID observe earlier operations in that batch. An empty or entirely absent-delete batch does not write or advance the sequence. A torn final batch frame replays none of its members.

| File | Role |
| --- | --- |
| `snapshot.bin` | Authoritative versioned config, metric, sequence, original records, and CRC32 |
| `wal.bin` | Checksummed single-change and atomic-batch frames |
| `index.bin` | Disposable graph cache bound to the snapshot checksum and sequence |
| `LOCK` | Exclusive OS lock held until the handle is dropped |
| `snapshot.tmp`, `index.tmp` | Temporary checkpoint files |

A successful write synchronizes the WAL before changing memory. A checkpoint synchronizes and renames a new snapshot and graph cache, synchronizes the directory, then clears and syncs the WAL. Opening restores a valid cached graph and inserts new WAL nodes. Missing, stale, or damaged caches rebuild from the authoritative records and record the reason in `RecoveryInfo`. Reopening without a checkpoint can still require substantial WAL replay and graph construction.

On Windows, directory synchronization uses a writable directory handle opened with `FILE_FLAG_BACKUP_SEMANTICS` through the standard library. Synchronization errors propagate; WAL truncation does not silently bypass them. Use an application-owned directory on a local filesystem. NTFS on Windows Server 2025 is exercised in CI; other filesystems, network shares and hardware power loss have not been validated.

Recovery trims only an incomplete final WAL frame at EOF. A complete checksum error, invalid record or sequence, damaged snapshot, or missing WAL fails opening. After a write or checkpoint I/O failure, the handle refuses further writes. Reopen and inspect the affected IDs: an operation that returned an I/O error may have persisted. CRC32 detects accidental damage; it does not authenticate files.

`checkpoint` bounds replay work. `compact` rebuilds from active records and reclaims tombstones. `maintenance_status` reports the WAL size and tombstone ratio. `maintain(MaintenancePolicy::default())` compacts at ≥128 tombstones and ≥20% tombstones; otherwise it checkpoints at ≥64 MiB of WAL. Maintenance runs synchronously when called; dropping a handle does not checkpoint.

`backup(new_directory)` checkpoints and copies a synced, independently openable snapshot/cache with an empty WAL. It refuses an existing destination. A failed copy may leave an incomplete destination, and backup changes the source's checkpoint state. Do not copy live database files individually.

Use a local filesystem and keep all database files together. Tests cover process kills, truncated WAL frames, and injected corruption. Hardware power loss, network filesystems, and secure erasure are not validated. Compaction temporarily holds a replacement index and can increase peak memory use.

## Documents and collections

Use `Collection` to keep original text, source paths, line ranges and tags with vectors. Each named collection binds its model identity, dimensions and metric; opening with a different embedding space fails. Source/tag filters use maintained posting lists. See the [collection guide](docs/collections.md) and runnable `collections` example. The underlying snapshot/WAL format is unchanged.

## Search real documents

The [Markdown search demo](examples/documents/README.md) uses a local Ollama embedding model, stores source paths and line ranges, and prints original chunks for natural-language queries. Python uses only its standard library; the Rust database keeps zero external dependencies.

## CLI

```bash
vecnook init data/demo 3 --metric cosine
vecnook put data/demo 1 1,0,0 "tenant-a"
vecnook put data/demo 2 0,1,0 "tenant-b"
vecnook search data/demo 1,0.1,0 10 128 auto --metadata "tenant-a"
vecnook checkpoint data/demo
vecnook stats data/demo
vecnook backup data/demo data/demo-backup
```

CLI metrics are `l2`, `cosine`, and `ip`. CLI search defaults to `auto`; specify `hnsw` or `exact` to force a mode. The metadata option compares the complete string for equality. Supply K, efSearch, and mode before `--metadata`. Append `--json` for machine-readable search reports with original metadata and full-precision distances.

For bulk writes, `vecnook batch data/demo changes.tsv` accepts tab-separated rows:

```text
put<TAB>3<TAB>0.9,0.1,0<TAB>tenant-a
delete<TAB>2
```

Replace `<TAB>` with a literal tab. Metadata is optional and may contain tabs, but not newlines in this format. The input text limit is 16 MiB; transaction limits below still apply.

`vecnook shell data/demo` keeps one handle open. Enter commands without the database path, then `quit`. For `put`, the remainder after the vector is metadata without quotes. Other shell arguments split on whitespace; use the standalone CLI or library for paths and metadata values containing spaces. `get`, `delete`, `compact`, `maintain`, and `--help` are also available. Standalone failures exit 1; shell failures print `ERR` and leave the shell running.

## Resource limits

| Resource | Limit |
| --- | --- |
| Dimensions | 1–4096, fixed per database |
| Coordinates | Finite `f32`; cosine requires a nonzero vector |
| IDs | Full `u64` range, unique among active records |
| Metadata | 16 KiB of UTF-8 per record |
| Physical nodes | 100,000, including old and deleted nodes |
| Snapshot | 256 MiB |
| Graph cache | 128 MiB |
| M / efConstruction / efSearch | 2–64 / M–4096 / 1–4096 |
| Atomic batch | 1024 operations and 8 MiB WAL payload |

The node and snapshot limits both apply; dimension and metadata size may make the byte limit bind first. `stats.vector_bytes` counts coordinates only, not total RSS. The WAL has no hard total-size cap; applications must call maintenance. The development [operating-range guide](docs/operating-range.md) measures vector workloads through 50,000 records and document workloads through 10,000, including filter, update, recovery and memory costs. Larger configurations remain uncharacterized.

## Compatibility and validation

0.3 retains the 0.2 snapshot/WAL format and reads 0.1 L2 snapshots and single-change WAL frames. Versions 0.2 and 0.3 write v2 snapshots and batch WAL frames that 0.1 cannot read. Typed collections add a separate immutable header and document payload schema; 0.2 does not expose those typed APIs. Keep a copy of all database files before upgrading; downgrade requires that copy. The 0.2 package/executable rename from `vector` to `vecnook` and frame-based recovery counters still apply.

```bash
cargo test --offline
cargo fmt --check
cargo clippy --offline --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --offline --no-deps
cargo package --offline
cargo tree --offline
```

Tests cover all three metrics against independent distance oracles, filtered Top-K, batch preflight and every byte boundary of a torn batch, graph cache validation and fallback, backups, file locks, v1 upgrades, and 900 model-checked mutation/maintenance/restart steps. Separate processes are killed after acknowledged writes and during observed WAL growth, checkpoint and compaction. CI runs on Linux, macOS and Windows with stable Rust and the declared 1.89 MSRV, including the document client and application-backend restart.

The [0.3 text benchmark](docs/text-benchmarks.md) uses 5,183 SciFact document embeddings, 768 dimensions and 300 independent test claims. At efSearch=64, Vecnook measured 99.7667% Recall@10 and 0.534209 ms median p95 across three runs on an Apple M4 Pro. An independent f64 oracle agreed with every exact Top-10 ID/distance. A same-machine USearch Python comparison met the same ≥99% target and was faster; runtime and memory scopes differ. See the full method and limits, plus [historical SIFT measurements](docs/benchmarks.md), before drawing broader conclusions.

Vecnook currently has no SIMD kernels, quantization, mmap storage, embedding model, network API, replication, or multi-process readers. See [CONTRIBUTING.md](CONTRIBUTING.md) to report reproducible problems or contribute.

Application guides: [API compatibility](docs/compatibility.md), [complete Markdown app](examples/documents/README.md#complete-application-flow), [incremental sync](examples/documents/README.md#incremental-updates), [diagnostics and restore](docs/operations.md), and [choosing an engine](docs/choosing.md).
