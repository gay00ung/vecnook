<div align="center">

# Vecnook

**An embedded vector database for Rust.**

HNSW search, exact search, and durable storage. Built with the standard library, with zero external crates.

[![CI](https://github.com/gay00ung/vecnook/actions/workflows/ci.yml/badge.svg)](https://github.com/gay00ung/vecnook/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/badge/crates.io-vecnook-orange.svg)](https://crates.io/crates/vecnook)
[![MIT license](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![XERJ Corpus Hub contributor](https://hub.xerj.org/contributors/gay00ung/badge.svg)](https://hub.xerj.org/contributors/gay00ung)

[Quick start](#quick-start) · [API reference](https://docs.rs/vecnook/1.4.0/vecnook/) · [Documentation](#documentation) · [Benchmarks](docs/current-benchmarks.md)

</div>

<p align="center">
  <a href="docs/media/offline-demo.mp4">
    <img src="https://raw.githubusercontent.com/gay00ung/vecnook/main/docs/media/offline-demo.gif" alt="Vecnook CLI demo: three prepared queries return Markdown source paths and line ranges; later queries reopen the saved collection." width="960">
  </a>
</p>

<p align="center">
  <em>Three prepared queries. Local Markdown sources. Data that survives a restart.</em><br>
  Recorded CLI results, with faster transitions and highlighted search steps. Uses precomputed embeddings.<br>
  <a href="docs/media/offline-demo.mp4">Watch the full recording</a> · <a href="docs/quickstart.md">Run it yourself</a>
</p>

## Why Vecnook?

- **Embed it in your app.** Use a Rust library or a local CLI on macOS, Linux, and Windows.
- **Choose how you search.** Exact or HNSW search with L2, cosine, and inner product metrics, plus metadata and document filters.
- **Keep your data across restarts.** Synced writes, atomic batches, checkpoints, recovery, and backups.
- **Search your documents.** Store original text and source references alongside application-provided embeddings.

Vecnook targets bounded, local datasets with one mutable database handle. The mutable HNSW index stays in RAM; embeddings come from your application. See [resource limits](#resource-limits) and [choosing an engine](docs/choosing.md) to check whether it fits your workload.

<a id="get-started"></a>

## Quick start

Requires **Rust 1.89+**. The examples below use the stable **1.4.0** release.

<a id="embed-a-database"></a>

### Rust library

```bash
cargo add vecnook@=1.4.0
```

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

### Command line

```bash
cargo install vecnook --version '=1.4.0' --locked
vecnook demo data/first-demo
vecnook demo data/first-demo 1
```

Use a fresh database path on the first run. For a compiler-free installation, download a ZIP and its SHA-256 file from the [1.4.0 release](https://github.com/gay00ung/vecnook/releases/tag/v1.4.0) and follow the [installation guide](docs/user-experience.md#install-a-native-cli).

<a id="search-real-documents"></a>

### Search a folder of Markdown

With the CLI installed, **Python 3.10+** and a local **Ollama** model:

```bash
ollama pull embeddinggemma
vecnook docs-sync ./my-markdown --db data/notes
vecnook docs-find "How do I recover my documents?" --db data/notes
```

Unchanged files are skipped on later syncs. Results include original chunks, source paths, and line ranges. Start with the [document walkthrough](docs/quickstart.md#search-your-own-folder) for model setup, filters, and ranking options.

## Documentation

| I want to… | Start here |
| --- | --- |
| Run a prepared offline demo | [Quick start](docs/quickstart.md) |
| Integrate with a Rust app | [API guide](docs/api.md) · [Worker and event-loop integration](docs/app-integration.md) |
| Build document search | [Collections](docs/collections.md) · [Complete example](examples/documents/README.md) |
| Filter and rank results | [Compound filters](docs/compound-filters.md) · [Hybrid search](docs/hybrid-search.md) |
| Back up, restore, or upgrade | [Operations](docs/operations.md) · [Compatibility](docs/compatibility.md) |
| Use immutable readers or SQ8 | [Generation API and optional acceleration](docs/engine-generations.md) |
| Evaluate performance and fit | [Benchmarks](docs/current-benchmarks.md) · [Operating range](docs/operating-range.md) · [Engine choices](docs/choosing.md) |

## Measured search performance

[![Vecnook 1.5.0-rc.1 SciFact benchmark: measured p95 search latency and Recall@10 across six candidate-pool sizes, with three fresh processes per setting.](https://raw.githubusercontent.com/gay00ung/vecnook/main/docs/media/search-benchmark.png)](docs/current-benchmarks.md)

Measured on **2026-10-29 with Vecnook 1.5.0-rc.1**, built from `main`. The chart shows the median of three fresh processes per setting; latency whiskers show the observed minimum and maximum. The workload uses 5,183 SciFact vectors, 768 dimensions, and 300 queries on an Apple M4 Pro.

This measures in-memory HNSW agreement with exact cosine Top-10, rather than human-rated relevance. Embedding generation, file loading, and index construction are excluded; it does not measure the optional SQ8 or SIMD/mmap paths.

[Results, raw data, and reproduction](docs/current-benchmarks.md) · [Other workload measurements](docs/operating-range.md) · [Historical 0.3 comparison](docs/text-benchmarks.md)

## Releases

**Stable: 1.4.0.** Adds segmented immutable generations, separate-process pinned readers, and SQ8 candidates with original-f32 reranking. The optional `vecnook-accel 0.1.0` companion supplies SIMD and read-only mmap. Install it only when you need its explicit platform APIs:

```bash
cargo install vecnook-accel --version '=0.1.0' --locked
```

Both registered candidates completed independent 24-hour workloads, alongside eight core and eight paired companion registry checks. See [1.4 validation and limits](docs/validation-1.4.md).

<details>
<summary>Candidate preparation: 1.5.0-rc.2</summary>

This checkout prepares core `1.5.0-rc.2`, exact-core companion `0.1.1-rc.2` and Python `1.5.0rc2`. It adds opt-in [JSONL sessions, prepared SQ8 caches, and bounded code buffers](docs/engine-generations.md#batch-and-session-cli-unreleased). P2 bounded-window mmap is excluded.

[RC1 is published](https://github.com/gay00ung/vecnook/releases/tag/v1.5.0-rc.1). Its completed source QA and distribution retain their original identities. The additional packaged README benchmark/demo tools require a new candidate fingerprint and RC2's own registry validation; RC1 receipts do not approve those additions.

Build this checkout with `cargo build --locked --release`. Once the exact RC2 registry versions are public, candidate installations use:

```bash
cargo add vecnook@=1.5.0-rc.2
cargo install vecnook --version '=1.5.0-rc.2' --locked
cargo install vecnook-accel --version '=0.1.1-rc.2' --locked
```

The optional companion and the separately downloadable `vecnook-1.5.0rc2-py3-none-any.whl` are candidate artifacts. The Python wheel needs the native CLI. [Development measurements](docs/validation-1.5.md) cover startup, repeated requests, whole-process RSS and the bounded-mmap decision; [candidate QA](docs/qa-1.5.md) records the independent registry/platform/full24h gates required before stable 1.5 promotion.

</details>

<details>
<summary>Earlier releases and validation</summary>

The [1.3 document workflow](docs/user-experience.md) and its [validation](docs/validation-1.3.md) remain available. Earlier memory and filter results are in [1.2 validation](docs/validation-1.2.md). See the [changelog](CHANGELOG.md) for release history.

</details>

## Technical reference

The details below cover search behavior, storage guarantees, CLI commands, and hard limits.

<details>
<summary>Search and filtering</summary>

### Search and filtering

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
| `Auto` (default) | Exact scan when eligible count ≤256, K exceeds efSearch, or a filter selects ≤1/8 of active records and ≤4096 candidates; otherwise HNSW, with exact repair if too few candidates are found |

`search_metadata` uses a maintained inverted index for exact equality of the complete metadata string. Only matching hash-bucket candidates are compared, with full string checks to reject collisions. `search_ids` accepts an application-selected subset and skips other records when preparing eligibility. Unfiltered `search` does not evaluate a predicate across all records. These indexes are rebuilt from authoritative records on open and updated through mutations and compaction.

`search_filtered` evaluates an arbitrary predicate once per active record, which costs O(active records). Ineligible/deleted nodes can be traversed but are not returned. Auto scans small eligible sets exactly. `filter_evaluations` reports predicate/metadata comparison counts; it does not count every graph operation. Graph traversal initializes one visited bit per physical node, local to the query. A predicate is not an access-control mechanism.

Results expose `mode`, `reason`, `eligible_count`, `distance_computations`, and `complete`. **`complete=true` means min(K, eligible count) results were returned; it does not certify recall.** Auto repairs a missing result count, not inaccurate full-length approximate results. K=0 returns no neighbors; K above the eligible count is capped at that count. For forced HNSW, efSearch must be at least the capped K.

</details>

<details>
<summary>Writes, recovery, and maintenance</summary>

### Writes, recovery, and maintenance

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

Use a local filesystem and keep all database files together. Tests cover process kills, truncated WAL frames, and injected corruption. Hardware power loss, network filesystems, and secure erasure are not validated. Compaction moves live payloads into a replacement graph after persisting them; its old and replacement graph allocations can still overlap.

</details>

<details>
<summary>Documents and collections</summary>

### Documents and collections

Use `Collection` to keep original text, source paths, line ranges, tags and optional numeric/date attributes with vectors. Each named collection binds its model identity, dimensions and metric; opening with a different embedding space fails. Source/tag and [compound filters](docs/compound-filters.md) use maintained posting lists. Existing documents keep VDOC1; optional attributes use VDOC2 and require a 1.2 reader. See the [collection guide](docs/collections.md) and runnable `collections` example. The underlying snapshot/WAL format is unchanged.

</details>

<details>
<summary>CLI</summary>

### CLI

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

</details>

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

## Validation and contributing

CI runs on Linux, macOS, and Windows, including stable Rust and the declared 1.89 MSRV. The [1.4 validation report](docs/validation-1.4.md) includes full 24-hour workloads and Windows 11 ARM64 NTFS verification.

<details>
<summary>Compatibility history, test commands, and benchmark context</summary>

### Compatibility and validation

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

The opt-in generation API supports SQ8 and separate-process readers; SIMD/mmap
requires the optional companion and its file-immutability contract. Embeddings
remain application-provided. Network APIs, replication and multiple mutable
writers remain outside this release. See [CONTRIBUTING.md](CONTRIBUTING.md) to
report reproducible problems or contribute.

Application guides: [API compatibility](docs/compatibility.md), [complete Markdown app](examples/documents/README.md#complete-application-flow), [incremental sync](examples/documents/README.md#incremental-updates), [diagnostics and restore](docs/operations.md), and [choosing an engine](docs/choosing.md).

</details>

Vecnook is available under the [MIT license](LICENSE). Read [CONTRIBUTING.md](CONTRIBUTING.md) for contribution guidelines.
