# Engine growth decisions and 1.4 implementation

The 1.3 user workflow kept its storage limits and original f32 coordinates.
The 1.4 candidate now implements the segmented snapshot, SQ8 candidate scan,
optional SIMD/mmap and multiple-process reader work described below. See the
[implemented API and boundaries](engine-generations.md) and
[measurements/release status](validation-1.4.md). The wider data-model ideas at the
end remain future scope. Candidate implementation is separate from stable approval.

## Capacity and the current storage boundary

Both the 100,000 physical-node cap and 256 MiB snapshot cap apply. At 50,000
records and 1,536 dimensions, coordinates alone occupy 307,200,000 bytes, about
293 MiB, before original text or graph data. Raising only the node constant cannot
make this workload fit. Updates also retain physical graph nodes until compaction.

The implemented store streams authoritative originals into bounded immutable
segments and publishes a checksum-bound manifest through CURRENT. It creates a
separate directory and leaves the legacy WAL/graph intact. A full bulk publication
has one source sequence; it has no generation WAL to truncate. Legacy readers
fail closed on this format. The new API does not provide a segmented HNSW graph
or incremental update log. Those need their own design and validation before
becoming product claims. Fault points, process interruption, original backups
and a real 50k/1536-dimensional capacity fixture exercise this implementation.

## Quantization with an exact reference

`Snapshot::quantize` implements an optional symmetric per-vector SQ8 candidate
scan while retaining original f32 records for exact search/reranking. It exposes
code/parameter bytes; the comparison reports calibration and held-out recall,
code construction/open time, full peak RSS and original segment sizes. Int8
coordinate bytes are one quarter of raw f32 bytes, but retained originals add
storage and mmap residency can raise full RSS. Defaults remain unchanged. This
is not an on-disk compressed graph or an automatic capacity/performance policy.

## Scalar kernels and optional acceleration

The core retains scalar ordered f64 distances from original f32 coordinates.
The optional `vecnook-accel` companion implements runtime AVX2/NEON, scalar fallback
and read-only mapping, with unsafe code isolated from the core. Its SIMD kernels
retain left-to-right f64 additions and avoid FMA/reassociation. Extreme finite
coordinates, dimension tails, signed zero and independent exact oracles are
tested. The mmap constructor explicitly requires lifetime-long file immutability;
cooperative pins cannot stop unrelated writers. Measurements do not justify a
universal speedup or changing the default reader/kernel.

## Readers and maintenance

The single persistent owner still protects legacy snapshot/WAL/cache replacement.
Generation readers now pin an immutable file set and identity under a short
publication gate. New publications do not mutate old files. Reclamation checks OS
leases; process exit releases abandoned pins. Existing views retain their sequence
while refresh opens CURRENT. Multiple readers and one bulk publisher can coexist.
This does not add multiple mutable writers or weaken legacy directory ownership.
An incremental mutable MVCC API/concurrent graph compaction remains future scope.

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
