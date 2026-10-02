# Changelog

## 0.2.0 — 2026-10-03

An embedded beta for Rust applications, licensed under MIT.

- Added persisted squared L2, cosine, and negative inner product metrics. Cosine keeps original coordinates and caches norms; zero vectors are rejected.
- Added predicate filtering and Exact/Hnsw/Auto strategies. Reports include eligible count and execution reason. Auto uses exact search for small candidate sets and repairs insufficient HNSW result counts.
- Added ordered atomic Put/Delete batches with full input preflight, one synced WAL frame, and one sequence per batch.
- Added a checkpoint graph cache with snapshot binding, CRC, bounds, and structural validation. Missing or damaged caches rebuild from authoritative records.
- Added independent backups to new directories and explicit threshold-based checkpoint/compaction maintenance.
- Added bounded fvecs loading and file-based recall/latency evaluation.
- Added Linux/macOS CI with stable Rust and Rust 1.89, contribution instructions, and a reproducible bug report template.

### Breaking changes and storage compatibility

- Renamed the Cargo package, library, and executable from `vector` to `vecnook`.
- Added `metric` to `Config` and `BenchConfig`, and execution details to `SearchReport`. Use constructors/defaults when updating config literals.
- Renamed recovery counters to `replayed_frames` and `skipped_frames`; a batch counts as one frame.
- CLI search now defaults to Auto. Use `hnsw` explicitly to preserve forced approximate execution.
- Reads legacy v1 L2 snapshots and single-operation WAL frames. New snapshots use v2, and batches use a new WAL opcode; 0.1 cannot open these new formats. Keep an independent pre-upgrade copy before changing versions.

## 0.1.0 — 2026-10-02

Initial Rust standard-library implementation: squared L2, exact Top-K, HNSW, CRUD, WAL recovery, checksummed snapshots, file locking, compaction, a CLI shell, and synthetic benchmarks.
