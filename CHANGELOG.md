# Changelog

## 1.5.0-rc.2 — candidate preparation

- Include the accepted HNSW four-candidate distance-scoring improvement while preserving f64 coordinate order, distance bits, graph candidate order and search settings. Keep the original path below 32 dimensions.
- Preserve the [32-process before/after measurements](docs/engine-comparison.md) and 12,264-search equivalence checks under their actual development-binary identities, including shared-host variation and all outliers.
- Freeze the changed runtime and additional packaged benchmark/comparison/demo tools in a fresh candidate. Prepare core 1.5.0-rc.2, exact-core companion 0.1.1-rc.2 and Python 1.5.0rc2; preserve RC1 archives, receipts and healthy workloads without reusing their approval for RC2.
- Require fresh source checks and exact-registry core/mapped/session full-duration gates before stable promotion. Mark SciFact group-memory samples without live engine PIDs as unavailable, preserving original samples and separate native high-water measurements. P2 bounded-window mmap remains excluded.

## 1.5.0-rc.1 — 2026-10-29 (prerelease)

- Add bounded JSONL generation sessions and batches with one pinned view, explicit close, EOF/idle cleanup and recoverable request errors.
- Add explicitly prepared, checksummed SQ8 caches with source identity, atomic completion, directory quotas and reader leases. Scan codes through a default 1 MiB workspace, preserving original reranking and resident SQ8 results.
- Use exact bounded i32 accumulation in the new cached-code scan, with full i8-range and maximum-dimension equality tests. Keep the dependency-free safe core, existing formats and default searches.
- Record actual CLI, peak RSS, simultaneous-reader and failure-boundary checks in [development validation](docs/validation-1.5.md). Keep the bounded mmap experiment outside the public companion.
- Prepare the 0.1.1-rc.1 companion against exact core 1.5.0-rc.1 and the Python 1.5.0rc1 client metadata. Extend the mixed soak with prepared-cache/lease checks and add an independent full-duration session/cache workload.

## 1.4.0 — 2026-10-27

- Promote the checked segmented-generation, SQ8 and process-reader APIs, paired with optional `vecnook-accel 0.1.0` SIMD/mmap. Preserve the candidate's runtime, tests, tools, build settings, Rust 1.89 support, original f32 data and existing mutable-store/default-search contracts.
- Complete the exact core RC's 86,401.129295292-second workload with 189,174 acknowledged mutations, 89 restarts, 17 backups, 710 publications, 355 SQ8 checks and 17 generation backups. Its independent mapped reader completed 86,400.107458791 seconds, 138,567,541 checked records and 248 observed generations, bound to the final publisher receipt.
- Verify eight core and eight paired companion registry consumers on Linux, macOS, Windows Server NTFS and Windows 11 ARM64 NTFS with MSRV and stable. Update exact-version installation instructions and [validation evidence](docs/validation-1.4.md), retaining all latency/RSS samples and CLI open/build costs. External first-use observations remain unmeasured.

## 1.4.0-rc.1 — 2026-10-26

- Add a separate segmented immutable store with streamed original-f32 publication, checksummed manifests/CURRENT, process-reader pins, explicit generation refresh and safe garbage collection. Database/Collection can publish acknowledged views; existing formats, exclusive legacy ownership and default search remain compatible.
- Add reusable symmetric SQ8 candidate scans with bounded pools, original-vector exact reranking, ID eligibility, original backup and `snapshot-*` CLI commands. Keep originals and distinguish result-count completeness from ANN recall.
- Add the optional `vecnook-accel 0.1.0-rc.1` companion: runtime AVX2/NEON with scalar-equivalent ordered f64 arithmetic, explicit immutable-file mmap safety boundary, native demo/search CLI and process-reader tests. Unsafe code stays outside the dependency-free core.
- Add actual publisher interruption/I/O/corruption coverage, paired original-oracle/recall/latency/full-RSS/capacity measurements, dual-package registry consumers and fresh generation/SQ8/mapped-reader soak gates. [1.4 validation](docs/validation-1.4.md) records measurements and the subsequent registered-candidate QA; 1.3 receipts do not approve this candidate.

## 1.3.0 — 2026-10-21

- Promote RC2's document previews, embedding reuse, explanations, optional Korean matching, bounded context/grouping, JSONL exchange and application-worker operations. Include its English BM25 ranking correction and reduced keyword-index construction allocations.
- Complete the exact registered RC2's 86,400.650580-second mixed workload with 189,268 acknowledged mutations, 89 restarts and 17 independent backup verifications. Eight CI registry consumers and a local Mac consumer passed, including Windows 11 ARM64/NTFS with Rust 1.89 and stable.
- Preserve the candidate's normalized runtime, tests, tools and build settings. Update exact 1.3.0 installation instructions and [validation results](docs/validation-1.3.md), including all RSS samples and the additional sustained search measurements. External first-use observations remain unmeasured.

## 1.3.0-rc.2 — 2026-10-20

- Fix the optional Korean analyzer's BM25 length normalization: added grams no longer inflate original document lengths. Queries without Hangul syllables retain the default word scores and order in mixed-language collections.
- Reduce keyword-index construction allocations and spare posting-tree capacity on reopen and analyzer changes. Invariant checks rebuild the selected analyzer once. Original text, f32 vectors, default results and persisted formats remain compatible; see [the measured comparison](docs/validation-1.3.md).
- Supersede RC1 and require new registry consumers and a fresh candidate-bound 24-hour workload before stable promotion.

## 1.3.0-rc.1 — 2026-10-20

- Added read-only document/vector inspection and sync dry-run plans without recovering or modifying database files. Unchanged embedding inputs reuse original stored vectors, including metadata-only edits.
- Added environment checks, document path aliases, per-command help, structured error opt-in, explicit empty-result messages and search explanations.
- Added opt-in Korean word grams, structural Markdown chunking, UTF-8 text input, source grouping, bounded adjacent context and distance bounds. Default tokenization and retrieval selection remain unchanged; context retains filters.
- Added queued keyword/hybrid queries, live queue activity, owning-handle diagnostics, compaction, policy maintenance and named backups.
- Added bounded JSONL exchange, new-path model reindex with source verification, a standard-library Python subprocess client and an asynchronous Node.js example.
- Added native CLI archive/checksum CI and expanded correctness/relevance checks. Candidate distribution and 24-hour stable-release gates must complete before stable promotion.


## 1.2.0 — 2026-10-16

- Promote the checked `1.2.0-rc.1` memory/update improvements and compound document filters to stable, retaining the standard-library-only Rust core and Rust 1.89 support.
- Complete the registered archive's 86,401.130573-second mixed workload with 189,283 acknowledged mutations, 89 restarts and 17 independent backup verifications. Eight exact-registry consumers pass on Linux, macOS, Windows Server and Windows 11 ARM64/NTFS with Rust 1.89 and current stable.
- Preserve the candidate's normalized runtime, tests, tools and build settings. Provide exact stable installation instructions, [measured results](docs/validation-1.2.md) and [reader migration guidance](docs/compatibility.md#migrating-from-11-to-12).

## 1.2.0-rc.1 — 2026-10-15

- Add indexed AND/OR/NOT expressions, finite numeric intervals and Gregorian date intervals to vector, keyword and hybrid document search. Preserve source/all-tag conditions and add the owned-filter app-worker method.
- Add bounded Number/Date metadata and opt-in VDOC2 payloads. Default documents keep byte-identical VDOC1; 1.2 reads existing files, including actual 1.1 snapshots/WAL/cache. Attribute-bearing documents require a 1.2 document reader.
- Add `--where` to document search commands and `--record-metadata` for Markdown file byte counts and UTC modification dates, with inferred repeat sync and explicit opt-out.
- Stream snapshots/caches, move live payloads during durable compaction, release inactive payload strings, pack search scratch bits, update repeated-ID batch postings once and clone only returned RRF documents.
- Compare fresh-process memory/update/search measurements with registry 1.1.0; add independent compound-filter oracles, streaming fault coverage and candidate-bound distribution/long-soak checks. [Measurements and validation status](docs/validation-1.2.md) distinguish local checks from the required full-duration registry QA.

## 1.1.0 — 2026-10-15

- Promote the checked `1.1.0-rc.1` document-search implementation to stable. Runtime source, tests, tools and build settings match the registered candidate after normalizing the package version; the existing 1.x API, JSON v1 and storage/model contracts remain compatible.
- Complete the registered archive's 86,401-second mixed workload with 189,284 acknowledged mutations, 89 restarts and 17 independent backup verifications. Exact-registry consumers pass on Linux, macOS, Windows Server and Windows 11 ARM64/NTFS with Rust 1.89 and current stable.
- Provide exact stable-version installation instructions and the measured search/first-use results in [1.1 validation](docs/validation-1.1.md). BM25/RRF, selective-filter Auto search and installed folder commands are described below.

## 1.1.0-rc.1 — 2026-10-14

- Add standard-library BM25 document postings and RRF hybrid search with indexed source/tag filters, branch ranks/scores and deterministic ties. Rebuild postings from existing original payloads after restart, backup and import; preserve API/JSON v1 and storage compatibility.
- Select exact vector search automatically for filtered sets of at most 4096 candidates and at most 1/8 of active records. Report `SelectiveFilter`; allow callers to disable the new threshold and preserve forced HNSW.
- Include the Python standard-library Markdown client in the CLI. `docs-sync` imports/updates a folder and `docs-find` infers model settings. Automatic text selection uses hybrid for explicit identifiers and vector ranking for prose; all modes remain selectable.
- Compare vector/BM25/RRF/automatic selection with fixed English/Korean questions and an error-code catalog. Extend the mixed workload to check lexical/fused ranks, selective Auto, document deletion, restart, independent backups and logical restore.

## 1.0.0 — 2026-10-09

- Preserve the public API, JSON v1 and storage/model contracts from `1.0.0-rc.1`; runtime source, tests and build settings remain identical after normalizing the package version.
- Provide exact stable-version installation instructions. Publication requires the completed candidate-bound distribution, Windows 11 NTFS and 24-hour/100k-mutation evidence described in [candidate validation](docs/release-readiness.md).

## 1.0.0-rc.1 — 2026-10-07

This candidate freezes the public API, CLI JSON v1 and storage/model compatibility contracts. Stable 1.0 publication additionally requires candidate-bound registry checks on all six OS/toolchain pairs, a completed 24-hour/100k acknowledged-mutation workload and Windows 11 NTFS installation/restore evidence. External adoption remains unmeasured.

- Complete document-folder sync/query/backup/restore and Rust application write/query/checkpoint/restart workflows; verify fresh packaged consumers and exact registry CLI/library resolution.
- Measure 13 operating-range cases with repeated independent processes, filtered/churn workloads and a pinned USearch Rust comparison. Expose p99 timings and constant-time capacity reports; avoid ID-subset preparation when collection filters cover every active document.
- Add feature and usage feedback forms, an engine-selection guide and an evidence-based release procedure.

- Validate storage failure points, seeded parser mutations and actual 0.1/0.2/0.3 upgrade files; add a mixed-workload runner with independently replayed ACK logs.

- Add read-only diagnostics, snapshot/node headroom, bounded logical export/import and collection backup commands.

- Add paginated documents, optimistic batch writes and source-atomic Markdown sync with explicit namespace-restricted pruning.

- Prepare extensible constructors and typed error categories; publish the bounded app worker and CLI JSON v1 with decimal string IDs.

- Offline prepared-query CLI demo with actual EmbeddingGemma outputs, original Markdown, checkpoint/reopen and generation provenance, available after registry installation.

### Migrating from 0.3

Use configuration/filter builders and wildcard branches for non-exhaustive public enums. CLI JSON IDs and sequences are decimal strings. Import the public `vecnook::app` worker rather than copying the example helper. Existing database and collection files retain their formats; see [compatibility](docs/compatibility.md).

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
