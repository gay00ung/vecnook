# Changelog

## 0.4.0-beta.1 — unreleased

- Validate storage failure points, seeded parser mutations and actual 0.1/0.2/0.3 upgrade files; add a mixed-workload runner with independently replayed ACK logs.

- Add read-only diagnostics, snapshot/node headroom, bounded logical export/import and collection backup commands.

- Add paginated documents, optimistic batch writes and source-atomic Markdown sync with explicit namespace-restricted pruning.

- Prepare extensible constructors and typed error categories; publish the bounded app worker and CLI JSON v1 with decimal string IDs.

- Offline prepared-query CLI demo with actual EmbeddingGemma outputs, original Markdown, checkpoint/reopen and generation provenance. Requires a development checkout until this version is published.

## 0.3.0-beta.2 — 2026-10-06

- First crates.io release, with exact-version registry installation instructions and links to hosted API documentation.
- Retains the beta.1 Rust implementation and storage formats. The version and distribution documentation are updated; existing GitHub beta.1 source remains available.

## 0.3.0-beta.1 — 2026-10-06

- Windows persistence with explicit writable directory synchronization, stable/MSRV CI on three systems and a bounded Rust application query worker with restart coverage.
- Checked public API documentation, versioned Git installation and guarded registry publication workflow. Source package privacy checks exclude local plans, environments, datasets and generated files.

- Reproducible real-text embedding benchmark, independent cosine oracle and optional USearch comparison at a shared recall target. Added process interruption coverage for WAL writes, checkpoint and compaction.
- Typed document chunks, independently named collections, immutable embedding-space binding and indexed source/all-tag filters. Collection backups retain model identity.
- Local Markdown search demo with Ollama, source line ranges, model digest checks and restart persistence.
- CLI `search --json` with escaped UTF-8 metadata and full-precision distances.
- Unfiltered Auto search skips predicate scanning; exact metadata equality uses a maintained inverted index. Added explicit-ID subset search and filter evaluation counts.

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
