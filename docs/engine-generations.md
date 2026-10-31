# Segmented generations, SQ8 and optional platform readers

The 1.4 release adds a separate immutable bulk snapshot store. Applications can
publish an acknowledged database or collection view, then let several processes
read it while the publisher prepares another generation. Existing mutable
`Database`/`Collection` directories, WALs, exclusive ownership and default HNSW
search keep their current formats and limits. Use a new directory for this API.

## Publish and pin a view

```rust,no_run
use vecnook::{Config, EmbeddingSpace, Metric, generation::{PublishOptions, Publisher, Snapshot}};

# fn main() -> vecnook::Result<()> {
let mut publisher = Publisher::create(
    "data/generations", Config::new(3),
    EmbeddingSpace::new("my-model-v1", 3, Metric::SquaredL2),
)?;
let mut next = publisher.begin(1, PublishOptions::default())?;
next.add(1, &[1.0, -0.0, 0.0], "original text 원문")?;
next.add(2, &[0.0, 1.0, 0.0], "second original")?;
let reader = next.commit()?;
assert_eq!(reader.get(1)?.unwrap().vector[1].to_bits(), (-0.0f32).to_bits());
let exact = reader.search_exact(&[1.0, 0.0, 0.0], 1)?;
assert_eq!(exact.neighbors[0].id, 1);
// Another process can use Snapshot::open on the same store.
let same = Snapshot::open("data/generations")?;
assert_eq!(same.sequence(), 1);
# Ok(())
# }
```

Use `Publisher::open` to reopen its single writer. `Database::publish_generation`
and `Collection::publish_generation` stream their current acknowledged originals
and sequence; the destination config/model identity must match. The legacy source
still obeys its existing exclusive lock. Direct streaming through `begin/add/commit`
can create larger bulk snapshots without first fitting them into a legacy database.

Each `Snapshot` pins one complete generation and reports its source `sequence`.
`refresh()` returns a new reader at CURRENT; it leaves the old reader intact.
Publication is a complete replacement, so omit a record to remove it from the next
view. It is synchronous and has no incremental generation WAL or background writer.
Source sequences can repeat but cannot decrease. Model, dimensions and metric
cannot change within a store.

The safe core reads original records on demand through positional file I/O.
Opening validates the complete manifest, all segment checksums, finite original
coordinates and UTF-8 before returning. It reads the corpus during validation;
this is not constant-time or lazy startup. Record lookup still decodes an owned
original vector and metadata. Segments currently contain original records, without
an on-disk HNSW graph; generation searches scan originals or derived SQ8 codes.

## SQ8 candidates with original reranking

```rust,no_run
# fn search(reader: &vecnook::generation::Snapshot, query: &[f32]) -> vecnook::Result<()> {
let codes = reader.quantize()?;
let matches = codes.search(query, 10, 64)?;
println!("codes={} parameter_bytes={} original_distances={}",
    codes.code_bytes(), codes.parameter_bytes(), matches.original_distance_computations);
# Ok(())
# }
```

Build the reusable quantized view once per pinned generation. Each vector has
symmetric int8 coordinates, an f64 scale and an i64 squared norm. Search scans
those codes, keeps a bounded candidate pool, loads its original f32 records, then
computes exact distances and distance/ID ordering. Returned metadata remains
original. Selecting every eligible record gives the exact reference ordering;
a smaller pool can miss true neighbors even when `complete=true`.

K is at most 10,000; the candidate limit must be K..=65,536. The code allocation
is bounded to 512 MiB before allocation. Codes use one byte per coordinate plus
16 bytes of scale/norm parameters per record, excluding allocator/index overhead.
Original segments remain on disk. Code construction reads all originals and its
memory/time cost must be included. This is a candidate scan, not a compressed
HNSW graph. Use `search_ids` with application-selected IDs to constrain eligibility;
duplicates are deduplicated, absent IDs ignored, and excluded IDs cannot be returned.
Predicates and ID lists do not replace application authorization.

## Publication, reclamation and restore

| File | Meaning |
| --- | --- |
| `STORE` | Checksummed store format/config/model identity |
| `WRITER`, `GATE` | Exclusive publisher lease and short reader/publication gate |
| `CURRENT` | Checksummed generation ID and expected manifest checksum |
| `gen-*/MANIFEST` | Checksummed complete segment set and bounded ID/offset index |
| `gen-*/segment-*` | Immutable original f32/UTF-8 records with trailing CRC32 |
| `gen-*/LEASE` | Shared reader pin, released by the OS on process exit |

Commit syncs segments, the manifest and generation directory, then atomically
replaces CURRENT and syncs the store directory before returning success. Failures
can be ambiguous: close/reopen the poisoned publisher and inspect CURRENT before
retrying. Dropping an unfinished builder does not publish it. Reader opening never
recovers or rewrites files. Checksum/logical damage fails closed.

`collect_garbage()` removes only non-current generations with no reader pin.
Held `Snapshot`s and retained segment handles keep their files alive. Slow readers
can therefore retain substantial disk space. Unknown files and incomplete
directories without a lease are preserved. `Snapshot::backup(new_path, options)`
streams originals into an independently openable generation store; it refuses an
existing destination. Preserve failed destinations/logs for diagnosis and use a
new path for another attempt.

Publication defaults to 16 MiB segments and permits 64 KiB..=64 MiB, at most
1,024 segments, 1,000,000 records and a 64 MiB manifest. Existing vector dimensions
(1..=4,096), finite-coordinate and 16 KiB metadata limits still apply. These are
parser/allocation bounds, not measured operating-range recommendations. See
[1.4 validation](validation-1.4.md) for actual measured capacity and tradeoffs.

Use local application-owned storage. Process kills and injected I/O faults are
covered; hardware power loss, network filesystems and hostile external writers
are unvalidated. CRC32 detects accidental damage and does not authenticate data.
Older readers cannot open this new store; no legacy directory is modified in place.
Keep the legacy source/backup if older readers or downgrade are required.

## SIMD and memory mapping

The optional `vecnook-accel` companion supplies runtime-detected AVX2/NEON kernels
and Unix/Windows read-only mappings. Core arithmetic stays portable, safe Rust
with no Cargo dependencies. `AutoKernel` keeps scalar ordered f64 results and
uses a scalar fallback. Select it explicitly through `search_filtered` or SQ8's
`search_with_kernel`; existing default searches do not use it.

`mapped_snapshot` is an **unsafe** Rust API: every mapped segment and its length
must remain immutable until the reader drops. Use Publisher for all updates and
prevent unrelated writers from editing or truncating files. OS read-only mappings
do not enforce external immutability. The adapter retains the file and reader pin
through unmapping. Use safe positional I/O if the application cannot guarantee
this condition. Mapping can increase resident memory and does not guarantee a
latency improvement. See the [companion README](https://github.com/gay00ung/vecnook/tree/main/extensions/accel).

## CLI

```bash
vecnook snapshot-publish data/raw-database data/new-generations my-model-v1
vecnook snapshot-info data/new-generations
vecnook snapshot-get data/new-generations 1
vecnook snapshot-search data/new-generations 1,0,0 10 --sq8 64
vecnook snapshot-backup data/new-generations data/new-generation-backup
vecnook snapshot-gc data/new-generations
```

These additive commands return JSON v1 with original payloads and decimal-string
IDs, generation and sequence. `snapshot-publish` reads a raw Database; use the
Rust `Collection::publish_generation` API for collection identity/payloads.
The optional `vecnook-accel search STORE VECTOR K --sq8 N --mmap` selects the
platform kernel and explicitly opts into the mapping immutability contract.

The one-shot CLI validates the full store each time. With --sq8 and no prepared
cache, it builds codes again for that invocation. The latency tables in [1.4 validation](validation-1.4.md)
measure queries on a retained Rust view after open/build; use the library to reuse
that view across queries. The tables do not describe one-shot CLI latency.

## Batch and session CLI (1.5)

snapshot-session accepts UTF-8 JSONL on stdin. snapshot-batch reads the same
protocol from a file. Both open one Snapshot, optionally build one resident SQ8
view, and retain that generation until exit:

    vecnook snapshot-session data/new-generations --sq8 64 --idle-ms 60000
    vecnook snapshot-batch data/new-generations requests.jsonl --sq8 64

Each line contains one request with an already prepared embedding. These commands
do not run an embedding model:

    {"request_id":"q1","vector":[1,0,0],"k":10}
    {"request_id":"q2","vector":[0,1,0],"k":1,"ids":["1","18446744073709551615"]}
    {"request_id":"done","op":"close"}

The initial ready event reports generation/sequence, completed snapshot_opens,
sq8_builds, cache_hits, code_read_buffer_bytes and separate open_ms,
code_build_ms and cache_validation_load_ms. Search responses contain request_id,
search_ms, unchanged cumulative constructor counts and result, using the existing
generation-search JSON v1 shape. Search time includes scanning, original reranking
and constructing results; it excludes JSON transport. A parsed request's search
error retains its ID; malformed frames report a null request ID and leave the next
request usable.

One request executes at a time. A rendezvous channel permits one bounded waiting
frame, without an accumulating queue. Each line is at most 1 MiB; oversized lines
are drained before accepting the next line. Request IDs have at most 128 UTF-8
bytes, vectors at most 4,096 coordinates, and ID lists at most 65,536 entries.
K retains the existing 10,000 bound; SQ8 candidates must cover K. Duplicate/unknown
fields, non-finite numbers and malformed Unicode are rejected.

EOF, an explicit close or idle timeout releases the view and pin. --idle-ms
defaults to 60,000 and accepts 1..=86,400,000; it measures waiting for a frame,
not query execution. Fatal input/output failures exit with an error. Process
termination releases OS leases. A new CURRENT leaves active sessions unchanged:
open another session to choose it. There is no network listener or generation pool.

## Prepared SQ8 and bounded buffers (1.5)

Prepare codes explicitly in a dedicated directory outside the source store.
Its parent must exist:

    mkdir -p data/sq8-caches
    vecnook snapshot-prepare-sq8 data/new-generations data/sq8-caches/gen-2
    vecnook snapshot-session data/new-generations --sq8 64 --sq8-cache data/sq8-caches/gen-2
    vecnook snapshot-search data/new-generations 1,0,0 10 --sq8 64 --sq8-cache data/sq8-caches/gen-2

Rust callers use Snapshot::prepare_quantized_cache and Snapshot::quantize_cached
with QuantizedCacheOptions. CachedQuantizedSnapshot borrows its validated Snapshot.
Its search, search_with_kernel and search_ids methods share the resident candidate
ranking, exact original reranking and result ordering. The existing quantize()
API keeps its resident allocation.

Preparation encodes one original record at a time, syncs a private pending file,
renames it to DATA, then syncs the directory. A crash before rename announces no
cache. An error after rename can leave a complete file: reopen and validate before
retrying. Existing DATA is never overwritten. Original files remain untouched.

Identity binds canonical source path, OS, generation, sequence, full config/model/
metric, manifest entries, segment lengths/checksums and encoding version. A moved
store or different generation needs another cache. Every reader validates the
original Snapshot, then the full cache identity, CRC, code range and scale/norm
invariants. Cache hits perform no re-encoding. Validation and reads still cost time.
Missing/incomplete caches fail with preparation guidance; reads never create them
or silently switch to a resident allocation.

Search reads row-aligned blocks of at most 1 MiB by default and retains one bounded
global candidate heap. --buffer-bytes N with --sq8-cache (or with_buffer_bytes)
must fit one dimension+16-byte row and cannot exceed 16 MiB. The actual aligned
size is reported. Original f32 records are read for final candidates. The N×D code
heap is avoided; manifest indexes, requests/results and concurrent searches still
consume memory. Code bytes and buffer bounds are not process RSS limits.

Each managed directory has a hard 1 GiB logical-byte limit, including its 8-byte
LEASE and pending files, and at most 1,024 entries. --max-cache-bytes N sets a
smaller preparation limit; Rust readers can select it too. Quotas apply per
directory. Applications must explicitly retire old directories; there is no
cross-directory eviction. Logical codes retain the existing 512 MiB bound.

    vecnook snapshot-prune-sq8 data/sq8-caches/gen-2

Pruning takes the exclusive cache lease, refuses active readers and removes only
managed DATA/pending files. Foreign files and symlinks are rejected. The directory
and LEASE remain to prevent lock-inode replacement. Close readers, prune, then
explicitly prepare again, or use another directory. A damaged/missing LEASE needs
a fresh directory. Keep DATA/LEASE immutable outside these APIs and apply the
source store's access controls.

See [development measurements](validation-1.5.md) for startup, latency, whole-process
peak RSS, simultaneous readers and the bounded-mmap inclusion decision.
