# Engine growth decisions after 1.3

The 1.3 user workflow keeps current storage limits and original f32 coordinates.
This design records the next experiments required before increasing supported
capacity or changing concurrency. It does not announce those capabilities.

## Capacity and the current storage boundary

Both the 100,000 physical-node cap and 256 MiB snapshot cap apply. At 50,000
records and 1,536 dimensions, coordinates alone occupy 307,200,000 bytes, about
293 MiB, before original text or graph data. Raising only the node constant cannot
make this workload fit. Updates also retain physical graph nodes until compaction.

The next storage experiment should segment authoritative records and graph
allocations, bound per-segment parsing and retain checksums/versioning. A manifest
would identify a complete committed segment set; publication must order synced
segments, synced manifest replacement and WAL truncation so interrupted maintenance
can select one consistent generation. Legacy readers must fail closed on the new
format; migration should create a separate verified directory rather than edit the
only copy in place. Gates include every interruption boundary, ACK replay, byte
budget enforcement, independent export/restore and 768/1536-dimensional churn.

## Quantization with an exact reference

Evaluate an optional derived quantized candidate index while retaining original
f32 vectors for exact search and reranking. Report recall at the same target,
build/reopen time, full peak RSS and the bytes for originals plus derived data.
An int8 coordinate representation uses one quarter of raw f32 coordinate bytes,
but retaining both adds storage and does not imply a 75% process-memory saving.
Reject a default change unless a same-machine measured workload benefits after
including reranking and model-specific recall loss.

## Scalar kernels and optional acceleration

The core currently accumulates scalar f64 distances from original f32 coordinates.
First profile those kernels and copies at matched recall before selecting an
optimization. Compiler auto-vectorization can be evaluated without changing the
public distance contract. Explicit intrinsics and mmap APIs can require unsafe
code/platform libraries, so they belong in a separately reviewed optional layer
if adopted; the std-only safe core remains the portable reference. Preserve finite
extreme-coordinate checks, deterministic ties and independent exact oracles.

## Readers and maintenance

The single persistent owner prevents readers from observing partial replacement
of snapshot/WAL/cache files. Multiple process readers need immutable generations,
reader pinning, lifetime/reclamation rules and restart-safe stale-reader handling.
A reader must never mix a new snapshot with an old graph/identity. Prototype
read-only snapshot copies before weakening the existing lock. Read/write snapshot
isolation and concurrent compaction would need additional memory and a documented
consistency contract; adding a thread alone does not supply either.

## Data model and integration decisions

General strings/booleans, nested/list attributes, timezone timestamps, named vector
fields, sparse vectors and multivectors require explicit bounds, missing-value
semantics and a reader migration before becoming product scope. Prefer optional
adapters for PDF/HTML, normalization, SQLite joins and model runtimes. JSONL and
the caller-owned embedding interface provide the present exchange boundary.

The next investment depends on actual corpus size/dimensions, update frequency,
startup tolerance and whether users choose Vecnook for its std-only deployment.
The project has no measured external-user cohort for this release; repository
fixtures cannot establish those product preferences.
