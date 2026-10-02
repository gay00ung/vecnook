# Vecnook

A vector database prototype built with the Rust standard library. Squared L2 distance, exact Top-K search, HNSW indexing, and persistent storage are implemented in this repository, with no external crates or vector search libraries.

The current version provides a library and a CLI for a single local database handle. Persistent storage supports macOS and Linux; testing has been performed on macOS. The Cargo package, library, and executable are named `vector`.

## Quick start

Requires Rust 1.89 or later. The project has been tested with Rust 1.96.

```bash
cargo build --offline --release
cargo run --offline --example basic
```

Create a database in a new directory and run a search:

```bash
./target/release/vector init data/demo 3
./target/release/vector put data/demo 1 1,0,0 "First vector"
./target/release/vector put data/demo 2 0,1,0 "Second vector"
./target/release/vector put data/demo 3 0,0,1 "Third vector"
./target/release/vector search data/demo 1,0.1,0 2 64 hnsw
./target/release/vector search data/demo 1,0.1,0 2 64 exact
```

Both searches should return ID 1 first, with a squared L2 distance of approximately 0.01. Results include IDs, distances, metadata, the number returned, and the number of distance calculations. Results are ordered by distance, then ID. `init` refuses to overwrite an existing database.

## Keep a database open

Each standalone CLI command rebuilds the graph when it opens the database. Use the shell to keep one handle open across commands:

```bash
./target/release/vector shell data/demo
```

Inside the shell, omit the database path. For `put`, everything after the vector is metadata; enter it without quotes.

```text
put 4 0.9,0.1,0 New vector
put 1 0.8,0.2,0 Updated vector
search 1,0.1,0 2 64 hnsw
get 1
delete 2
stats
checkpoint
compact
quit
```

Putting an existing ID replaces its vector and metadata. Deleting an absent ID returns `absent` without writing a log entry. Getting an absent ID fails. Standalone commands exit with code 0 on success and 1 on failure; shell command failures print `ERR` and leave the shell running.

## Library

`VectorIndex` provides an in-memory index. `Database` adds persistent storage and recovery.

```rust
use vector::{Config, Database, Result};

fn main() -> Result<()> {
    let mut db = Database::create("data/app", Config::new(3))?;
    db.put(1, &[1.0, 0.0, 0.0], "First vector")?;

    let result = db.search_hnsw(&[1.0, 0.1, 0.0], 10, 128)?;
    println!("{:?}", result.neighbors);

    db.checkpoint()?;
    drop(db);

    let db = Database::open("data/app")?;
    assert!(db.get(1).is_some());
    Ok(())
}
```

Use `Database::open` for an existing database. Mutations require `&mut self`; reads use `&self`. Applications that share mutations and queries between threads must synchronize access, for example with `RwLock`. An immutable `VectorIndex` can be shared between readers.

## Choose a search mode

Exact search scans all active vectors and returns the exact nearest neighbors. Use it as a reference for evaluating approximate search or when scanning the dataset is inexpensive.

HNSW follows a graph to examine fewer candidates. Increasing `efSearch` generally improves recall at the cost of more work. Defaults are M=16, efConstruction=200, and CLI efSearch=128. Tune `efSearch` against exact results on your own data.

Deleted nodes remain available as graph paths but are excluded from results. Updates insert a new node and mark the previous node deleted. `compact` rebuilds the graph from active vectors and reclaims old nodes. If HNSW reaches fewer than the requested number of active candidates, it returns those candidates with `complete=false`. It does not silently switch to exact search.

## Storage and recovery

| File | Purpose |
| --- | --- |
| `snapshot.bin` | Versioned configuration, mutation sequence, original vectors, metadata, deletion state, and CRC32 |
| `wal.bin` | Upsert and delete records in a write-ahead log |
| `LOCK` | OS exclusive lock held while the database is open |
| `snapshot.tmp` | Temporary checkpoint file; ignored when a committed snapshot exists |

Each acknowledged mutation is written to the WAL and synchronized before it changes the in-memory index. A checkpoint synchronizes a temporary snapshot, renames it, synchronizes the directory, and then clears the WAL. Recovery skips WAL operations already included in the snapshot.

Recovery removes only an incomplete final frame at EOF and reports the number of discarded bytes. Complete checksum errors, invalid records or sequences, a damaged snapshot, or a missing WAL cause opening to fail. CRC32 detects accidental corruption. After an I/O failure, the handle refuses further mutations; reopen the database and check the affected ID to determine whether that operation persisted.

The graph is rebuilt from original vectors on every open. Closing a handle does not perform a checkpoint. Use a local filesystem and keep database files together. Process termination and partial WAL recovery are tested; hardware power loss and disk failure are not.

## Input limits

| Input | Limit |
| --- | --- |
| Dimensions | 1–4096, fixed per database |
| Coordinates | Finite `f32`; squared L2 is accumulated in `f64` |
| IDs | Full `u64` range, unique among active records |
| Metadata | UTF-8, up to 16 KiB |
| M | 2–64 |
| efConstruction | M–4096 |
| efSearch | 1–4096 and at least min(K, active records) |
| Physical nodes | Up to 100,000, including deleted nodes |
| Snapshot size | Up to 256 MiB |

K=0 returns no neighbors. K above the active count is capped at that count. Invalid input is rejected before recording a mutation. Run `compact` to reclaim space consumed by updates and deletions.

## Validation and benchmarks

```bash
cargo test --offline
cargo fmt --check
cargo clippy --offline --all-targets -- -D warnings
cargo tree --offline
./target/release/vector bench 10000 512 200 128 42 clustered
./target/release/vector bench 10000 512 200 32 42 clustered
./target/release/vector bench 10000 512 200 128 42 uniform
./target/release/vector bench 10000 512 200 512 42 uniform
```

Benchmark arguments are vector count, dimensions, query count, efSearch, seed, and dataset. Queries are generated independently from stored vectors. Recall@10 is the mean overlap with exact Top-10 results. Ten warmup queries are excluded. QPS measures sequential search calls; it excludes graph construction, disk writes, networking, and concurrent requests.

The initial release measurements used 10,000 vectors, 512 dimensions, 200 queries, seed 42, M=16, and efConstruction=200 on an Apple M4 Pro with 48 GiB RAM, macOS 26.6.2, and Rust 1.96.

| Synthetic dataset | efSearch | Recall@10 | HNSW p95 (ms) | HNSW sequential QPS | Exact sequential QPS |
| --- | ---: | ---: | ---: | ---: | ---: |
| Clustered | 128 | 100.00% | 0.126708 | 9,537.05 | 400.58 |
| Clustered | 32 | 100.00% | 0.104250 | 12,167.24 | 398.88 |
| Uniform | 128 | 72.45% | 1.265958 | 988.48 | 403.29 |
| Uniform | 512 | 95.95% | 3.208250 | 387.77 | 392.70 |

Clustered data used up to 64 centers with coordinate noise of ±0.04. Uniform data used coordinates in [-1, 1]. These are single-machine synthetic measurements, not results on real embeddings or identity recognition. For uniform data, increasing efSearch improved recall but removed the speed advantage over exact search. They do not establish an accuracy or throughput guarantee for other datasets.

The 45 automated tests cover exact results, input boundaries, graph invariants, updates and deletions, compaction, corruption handling, file locking, and recovery after killing a separate CLI process immediately after acknowledged mutations. Linux, Rust 1.89, and loads reaching the storage limits have not been tested.

## Source layout

| Source | Responsibility |
| --- | --- |
| [math.rs](src/math.rs) | Squared L2 distance |
| [index.rs](src/index.rs) | Exact Top-K, HNSW, records, updates, deletion, and compaction |
| [rng.rs](src/rng.rs) | Reproducible graph levels and synthetic fixtures |
| [storage.rs](src/storage.rs) | Binary format, CRC32, WAL, snapshots, and locking |
| [db.rs](src/db.rs) | Persistent mutation ordering and recovery |
| [main.rs](src/main.rs) | CLI and persistent shell |
| [bench.rs](src/bench.rs) | Independent-query recall and latency measurements |

Embedding generation, metadata filtering, quantization, an HTTP server, replication, and distributed operation are outside the current implementation.
