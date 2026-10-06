# Inspect, export and restore

These commands require the 0.4 development branch until it is published.

```bash
cargo build --release --offline
target/release/vecnook demo data/offline-demo
target/release/vecnook doctor data/offline-demo/demo
```

`doctor` returns JSON v1 containing model, dimensions, metric, active/physical
records, tombstones, current file sizes, estimated checkpoint size, remaining
snapshot/node budget and maintenance recommendations. It validates authoritative
snapshot/WAL records in memory under the same exclusive OS lock as the DB.
The existing `LOCK` is opened read-only; missing locks and active owners cause
failure. It never creates files, trims an incomplete WAL tail, checkpoints or
rebuilds a cache. `pending_tail_bytes` reports an incomplete final frame left
untouched. Complete checksum corruption fails closed. A missing/invalid derived
cache is reported separately. Normal `Database::open` performs recovery and may
trim a tail; use `doctor` when preserving evidence matters.

`Database::capacity()` and `Collection::capacity()` report the encoded snapshot
and node budget while the application owns its handle. `raw_vector_bytes` counts
coordinate payload only. Text, graph edges, allocation overhead and snapshot
buffers also consume memory; the count is not total process RSS. A write can hit
the 256 MiB snapshot cap before reaching 100,000 physical nodes. Replaced/deleted
versions still consume both budgets until explicit compaction. Checkpointing
reduces WAL recovery work but retains tombstones; compaction reclaims them.

## Logical export and import

The Rust API offers `Database::export/import` and `Collection::export/import`.
Exports contain active original records, fixed construction settings and, for
collections, the embedding identity and document payloads. IDs preserve all
64 bits, original f32 coordinates preserve their bit representation, and source,
text, tags and line ranges round-trip. Tombstones, graph cache and WAL history
are omitted; imports rebuild a fresh graph and begin at sequence zero.

For a vector database:

```bash
target/release/vecnook init data/raw 2
target/release/vecnook put data/raw 18446744073709551615 1,0 'original text'
target/release/vecnook export data/raw data/raw.export
target/release/vecnook import data/raw.export data/restored
target/release/vecnook search data/restored 1,0 1 128 exact --json
```

For document collections, use `docs-export` with the same identity arguments as
`docs-search`, then `docs-import <file> <root> <new-name>`. `docs-info <root> <name>`
provides the identity. Python's document sample also exposes an operations flow.
Collection names can change during logical import; model identity cannot.
`docs-backup` creates an independent named backup with its collection header.
The raw `backup` command operates on vector DB files and does not add a
collection header; use the collection backup method for documents.

The export format begins with `VECXPORT`, version 1, a raw/collection discriminator,
dimensions/M/efConstruction/seed/metric, a bounded model string, record count,
ID/f32/UTF-8 payload records, then CRC32. Numbers and coordinates are little-endian.
Files are bounded to 256 MiB + 1,024 bytes; imports also enforce the normal
snapshot, node and metadata limits, unique IDs, finite/model-compatible vectors,
valid original documents and no trailing fields. Future versions fail closed.
CRC32 detects accidental damage; it is not an authenticity signature.

Export files and import directories must be new. Import validates the complete
archive and builds its complete index before claiming the destination. It writes
one full snapshot, rather than exposing incremental partial imports. Storage
failure can leave an incomplete export/directory. Inspect that path; choose a
new path to retry, or remove only the failed artifact after confirming it contains
no needed data. The command never replaces an existing directory on retry.

## Backups and upgrade scope

Back up before upgrading, retain the original until the new copy passes exact
queries and document checks, and keep the current model/digest available. A
collection backup includes its identity and opens with the same expected space.
Logical import offers an independent restore path and validates all records.
Missing WAL, full-frame corruption and unknown authoritative versions are
failures requiring investigation; do not initialize over them. Graph caches are
disposable and can rebuild during a normal open. See [compatibility](compatibility.md)
and [storage guarantees](../README.md#writes-recovery-and-maintenance) for supported filesystem and failure scope.
