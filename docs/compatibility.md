# Compatibility contracts

Stable `1.0.0` preserves the public Rust API, CLI JSON v1 and storage/model
contracts validated in `1.0.0-rc.1`. A runtime change before stable publication requires a
new candidate and validation. Stable 1.x releases preserve compatible public APIs;
breaking API or required JSON changes require a new major version.

Construct configuration with `Config::new`, chain `with_*` methods, then call
`validate` or create an index. Use `SearchOptions::default()` and
`DocumentFilter::default()` with their builders. Public fields remain readable
and writable. Configuration, identity, document and result structs are
`non_exhaustive`: downstream code cannot construct them with struct literals.
Returned results are produced by the library. Match public enums with a wildcard
for future variants. Exported Rust types in `bench` follow the same SemVer policy.
Benchmark timings, human-readable reports, internal graph structure and Auto
selection heuristics may change; they are not stable numerical or output contracts.

`Error::kind()` distinguishes invalid input, capacity, embedding mismatch,
corruption, I/O, ownership, destination, write conflict and poisoned-handle failures. Capacity
reports the resource, limit and required value before any mutation. An I/O error
from a write can occur after persistence: close the handle, reopen, inspect the
affected IDs, and reconcile before retrying. The original I/O error remains
available through `std::error::Error::source`. Display text is for humans.

`vecnook::app::start` moves one collection onto a thread with a 32-request queue.
Client clones submit without blocking. `Busy` means no request was enqueued;
`Stopped` means the worker has ended. Accepted requests execute in queue order.
Dropping a result receiver does not cancel accepted work. Drop all client clones,
then join the worker to drain its queue and regain the collection. Never wait on
`recv` in a GUI event loop. Client count does not imply parallel searches.

`SearchReport.complete` means the requested number of eligible candidates was
returned, capped to the eligible count. Measure ANN recall separately. Auto uses
exact search for small eligible sets, large K and graph underfill, and reports
its decision. Forced HNSW can underfill; exact search supplies a reference result.

## CLI JSON v1

The 1.1 candidate adds keyword/hybrid methods and result types without changing
existing vector methods or file formats. `SearchOptions` adds
`filtered_exact_threshold` (default 4096) and its builder; set it to zero to
disable selective-filter Auto. `SearchReason::SelectiveFilter` is an additional
non-exhaustive variant. Keyword indexes rebuild from original payloads and require
no storage migration. New CLI commands have separate JSON v1 shapes; existing
`docs-search` and `search --json` retain their required fields and ordering.

`search ... --json`, `docs-info`, `docs-get`, `docs-list` and `docs-search` include
`"schema_version": 1`. IDs are canonical unsigned decimal **strings**, including
`"18446744073709551615"`. JavaScript can convert them with `BigInt(id)`; never
convert to `Number` for identity comparisons. Counts and line numbers remain
JSON numbers. Text, source, tags and model identity are UTF-8 strings, with JSON
escaping for quotes, tabs and newlines. Vectors remain original f32 values in
storage; reported distances are f64 numbers.

Search reports require `mode`, `metric`, `complete`, `eligible_count`,
`distance_computations`, `filter_evaluations`, `reason` and `neighbors`.
Neighbors require `id`, `distance`, `metadata`. Documents require `id`, `source`,
`text`, `start_line`, `end_line`, `tags`. Document search requires `search` and
`matches`; each match contains `distance` and `document`. Consumers must ignore
additional fields and reject unsupported schema versions. Exit 0 means the
command succeeded; exit 1 means failure, described on stderr. Human-readable
CLI output is outside the machine-readable contract.

## Storage and Rust support

MSRV is Rust 1.89. The 1.x policy retains that MSRV; a required increase is a
major-version change. CI exercises 1.89 and current
stable on Linux, macOS and Windows. The embedded core uses only Rust std.

Snapshot v2 reads v1 squared-L2 snapshots. WAL operation formats include atomic
batches; sequence numbers count committed frames. Collection header v1 and
`VDOC1` payloads preserve model identity and original documents. Graph cache v1
is derived: missing, stale or damaged caches rebuild from authoritative records.
Unknown authoritative versions and complete checksum damage fail closed.
Package SemVer and file-format versions are separate. Back up before upgrading;
do not expect an older binary to read files written by a newer unsupported format.
Future 1.x storage changes require a documented reader/migration path and fixtures.

## Migrating from 0.3

Replace `SearchOptions { strategy, ..Default::default() }` with
`SearchOptions::default().with_strategy(strategy)`. Replace
`DocumentFilter { source: Some("a.md"), tags: &["rust"] }` with
`DocumentFilter::default().with_source("a.md").with_tags(&["rust"])`.
Use builders for `Config` and `MaintenancePolicy` as well. Replace exhaustive
enum matches with a wildcard branch that handles an unsupported future value.
Import `vecnook::app` instead of copying the example worker. Treat CLI IDs as
strings. Existing collection and database files retain their current formats.

The reason for preparing these changes before 1.0 is explained by Cargo's
[SemVer guidance](https://doc.rust-lang.org/cargo/reference/semver.html) and
[MSRV reference](https://doc.rust-lang.org/cargo/reference/rust-version.html).

Document listing uses a decimal string `sequence`, a nullable decimal string `next_after`, and at most 256 `documents` per page. Keep the same sequence across pages. Pass it to `docs-batch ... --if-sequence N` to reject intervening writes.

## Migrating from 1.1 to 1.2

The 1.2 release preserves existing Rust methods, public fields, Document's Eq
contract, JSON required fields and low-level snapshot/WAL/cache formats. New
`Number`, `Date` and `FilterExpr` types and builders add compound filtering.
Source/tag-only documents retain byte-identical VDOC1 encoding. Adding numeric or
date attributes explicitly opts into VDOC2; 1.1 and older typed document readers
reject that version. Keep a pre-upgrade backup for downgrade, or remove attributes
using 1.2 into a separate checkpointed/exported collection first. JSON v1 includes
optional `numbers` and `dates` objects when nonempty; consumers ignore these
additional fields. See [the complete reader and range contract](compound-filters.md).

Deleted/superseded payloads may be empty in physical tombstone records. Those
records were already excluded from public active-record APIs. Original live
vectors, metadata and sequence ordering remain intact. Memory allocation details
and disposable graph structure are outside the compatibility contract.
Before using an older binary on 1.2-written VDOC1 data, checkpoint with 1.2 or
create a separate logical export/import. An uncheckpointed replacement-heavy WAL
may exceed the older reader's byte accounting because 1.2 reclaims inactive
payloads during replay. This does not change acknowledged active records or the
snapshot/WAL layout.

## Migrating from 1.2 to 1.3

Default tokenization, retrieval selection, original coordinates, VDOC1/VDOC2 and snapshot/WAL/cache formats remain unchanged. Optional Korean tokenization rebuilds only the runtime keyword index and must be reselected after open. Structural Markdown chunking creates a new collection identity with input schema 2; older convenience clients reject it. Native readers retain the opaque identity and original payloads. Existing CLI JSON v1 fields and positional commands remain supported. New optional context/group/distance selection is explicit and documents candidate versus final count completeness in the [1.3 guide](user-experience.md). The Python client is a separate standard-library subprocess package; it does not add core Cargo dependencies.
