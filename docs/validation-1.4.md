# 1.4 engine candidate validation

Local implementation and measurement date: 2026-10-26. Core `1.4.0-rc.1` and
optional companion `0.1.0-rc.1` are prepared from the same branch. Registry
publication, exact-registry cross-platform receipts and fresh full-duration
workloads are still pending. These local results do not approve stable release.
The completed 1.3 workload is historical evidence for 1.3 only.

The [generation guide](engine-generations.md) describes the opt-in APIs. Existing
mutable stores, HNSW/Auto defaults, original f32 coordinates and Rust 1.89 remain
compatible. Generation readers scan originals or SQ8 codes; they do not contain
a segmented HNSW graph or incremental mutation WAL.

## Method and scope

Measured on Apple M4 Pro, macOS ARM64, release builds with Rust 1.89. Each mode
runs in a fresh process, three times in rotated order, after fixture preparation
and calibration. `/usr/bin/time -l` records the full process peak RSS, including
open-time validation, resident mmap pages and SQ8 construction. This is process
RSS, not total system memory: the OS file page cache is not charged in the same
way as mapped resident pages. Open and code-build times are separate from query
latency; code construction is paid once per reusable quantized view.

The user workstation was not isolated. This measurement launched no overlapping
Vecnook builds, model inference or soak workloads; other projects and system
activity were uncontrolled. Documentation/tool edits overlapped,
and a 14 ms Python release-gate unit-test run overlapped the capacity collection.
Latency varied substantially in some runs; all samples below remain reported.
These results support workload-specific choices, not universal speed or RSS claims.
An equivalent fvecs decoder refactor after collection was checked against both
fixtures: query result checksums stayed identical; the timed query path did not change.

Each fixture uses 120 independent queries: the first 20 calibrate budgets and
the remaining 100 measure held-out ANN recall@10 and latency. The preparation
pass independently computes exact original-f32/f64 top-10 oracles, with ID tie
ordering. Ascending budgets 10,16,32,64,128,256,512,1024,2048,8192,65536 select
the first calibration recall of 1.0; held-out results never select the budget.
The validation target is recall >=0.99. This is ANN agreement with exact vectors,
not relevance judged by people. Modes are exact positional I/O (`file`), exact
mmap/scalar, exact mmap/SIMD, SQ8 with file or mmap originals (SIMD reranking),
and the unchanged legacy forced-HNSW baseline where its limits permit it.

The frozen SciFact inputs contain 5,183 768-dimensional EmbeddingGemma vectors
and 300 queries; the deterministic first 120 queries are used here. Their stored
provenance records the original BEIR source/model digests and prompt/truncation
settings. Engine fixture metadata is an authored row label, rather than the full
scientific abstract. Independent original-text preservation is checked separately
in API/backup tests. The capacity fixture is deterministic 64-cluster synthetic
50,000 x 1,536 data with seed 42 and independent noisy queries; it is not a real
50k-document user corpus.

## Frozen SciFact: 5,183 x 768

The store has 1 segment(s), 16,050,645 encoded original bytes. SQ8 candidate budget: 16. Legacy HNSW efSearch: 64.

| Mode | Min recall@10 | p95 ms, median | Full peak RSS MiB, median | RSS MiB, all three |
| --- | ---: | ---: | ---: | --- |
| file | 1.000 | 14.840 | 2.75 | 2.75 / 2.75 / 2.78 |
| mmap | 1.000 | 20.416 | 18.08 | 18.09 / 18.08 / 18.08 |
| mmap-simd | 1.000 | 9.564 | 18.09 | 18.09 / 18.08 / 18.12 |
| file-sq8 | 1.000 | 0.954 | 6.61 | 6.61 / 6.59 / 6.70 |
| mmap-sq8 | 1.000 | 0.925 | 21.95 | 21.94 / 21.95 / 22.06 |
| legacy | 0.999 | 0.581 | 22.20 | 22.20 / 22.33 / 22.20 |

| Mode | p95 ms, all three | Open ms, median | Code build ms, median |
| --- | --- | ---: | ---: |
| file | 14.840 / 26.999 / 13.012 | 36.979 | 0.000 |
| mmap | 9.278 / 31.444 / 20.416 | 66.615 | 0.000 |
| mmap-simd | 9.564 / 9.539 / 53.358 | 66.489 | 0.000 |
| file-sq8 | 0.944 / 0.954 / 3.927 | 34.716 | 11.062 |
| mmap-sq8 | 0.903 / 0.925 / 1.292 | 66.126 | 8.729 |
| legacy | 0.581 / 0.533 / 1.081 | 43.801 | 0.000 |

Derived SQ8 coordinates: 3,980,544 bytes; scale/norm parameters: 82,928 bytes. Originals stay on disk.

Input SHA-256: base `4995c03f6910c437ae5a60c8c62392af28eeee33c486e978de6b3216b11783fb`, queries `84735c492ff0e4c0e951e7f1d36cc728231192a497ccb2d93a2a0761edf6678e`. Oracle SHA-256: `557f1048c2eff553b0307953ee388d78d88b80e16c209c24d70779f65693cbab`.

## Capacity: 50,000 x 1,536

The store has 19 segment(s), 308,488,966 encoded original bytes. SQ8 candidate budget: 64. The legacy baseline is inapplicable: raw coordinates alone are 307,200,000 bytes, exceeding its unchanged 256 MiB snapshot cap.

| Mode | Min recall@10 | p95 ms, median | Full peak RSS MiB, median | RSS MiB, all three |
| --- | ---: | ---: | ---: | --- |
| file | 1.000 | 257.677 | 5.38 | 5.45 / 5.38 / 5.28 |
| mmap | 1.000 | 202.929 | 299.62 | 299.62 / 299.62 / 299.58 |
| mmap-simd | 1.000 | 202.737 | 299.62 | 299.55 / 299.62 / 299.67 |
| file-sq8 | 1.000 | 16.270 | 79.42 | 79.53 / 79.42 / 79.42 |
| mmap-sq8 | 1.000 | 16.161 | 372.98 | 372.98 / 372.97 / 373.73 |

| Mode | p95 ms, all three | Open ms, median | Code build ms, median |
| --- | --- | ---: | ---: |
| file | 531.736 / 241.024 / 257.677 | 715.104 | 0.000 |
| mmap | 208.781 / 184.920 / 202.929 | 1282.149 | 0.000 |
| mmap-simd | 217.237 / 190.770 / 202.737 | 1289.131 | 0.000 |
| file-sq8 | 16.270 / 16.223 / 16.387 | 671.054 | 208.612 |
| mmap-sq8 | 16.033 / 16.161 / 16.334 | 1284.086 | 161.755 |

Derived SQ8 coordinates: 76,800,000 bytes; scale/norm parameters: 800,000 bytes. Originals stay on disk.

Input SHA-256: base `fca7f8ba2593c502da708a7cfc59b4165779e6313df2f31070c639fbe8279d6f`, queries `3ddd741c0cd26e6fd9daa5acb8c9146be26765485186fe4289e6bffaa748657b`. Oracle SHA-256: `fa4e7faa33989f1039b5bab8104472cb3a57f99e7158de4f20c1a2fce8c63a70`.

## Interpretation

The file-backed SQ8 view traded extra code memory/build time for much fewer
original-vector reads. On SciFact its median p95 was 0.954 ms at recall 1.0, while
legacy HNSW was faster at 0.581 ms and recall 0.999. Retain the existing HNSW
choice when it fits and its measured recall is sufficient. The large segmented
fixture completed beyond the legacy snapshot cap, without raising that cap.
Its file-backed SQ8 view used 79.42 MiB median full RSS and 16.270 ms p95.

Mapping made original pages resident: the large mmap+SQ8 runs reached
372.98 MiB median and 373.73 MiB in the highest run. File-backed originals plus
SQ8 can suit a lower process-RSS budget; mmap is an explicit latency/access
choice with a larger resident footprint. Full-RSS and system-cache accounting
prevent interpreting the one-byte codes as a 75% total-memory reduction. SIMD
kept scalar results but supplied little stable benefit over mapped scalar in
the large scan. No new default is selected from these measurements.

## Correctness and remaining gates

Local MSRV tests cover original signed-zero/full-u64/UTF-8 values; all three
metrics against independent exact oracles; SQ8 full-candidate equality and
filtered IDs; model/sequence contracts; retained old readers and garbage
collection; independent backups; actual writer/reader process interruption;
CRC/logical corruption; and nine injected publication I/O failure boundaries.
The optional adapter's macOS ARM64 release tests cover NEON/scalar bit equality,
dimension tails/extreme finite values, threaded mapped reads and killed process
pin release. The CI matrix must additionally exercise AVX2 and Windows mapping.
Hardware power loss and network filesystems remain unvalidated.

Required stable gates remain separate: all eight exact core registry consumers,
all eight exact paired companion consumers, a new completed 24-hour/100k-mutation
core generation/SQ8 workload and a separate completed 24-hour/100k-record mapped
reader observing the same live publisher. Both candidate source fingerprints and
archives must match the promotion. See [paired release gates](release-readiness.md#paired-14-engine-gates).
A short smoke, partial/interrupted run, source-only test or older release receipt
is never a substitute. External five-person observation and actual app adoption
remain unmeasured because no participants/apps were available.

Local prepublication checks completed: Rust stable core 161 passed (three subprocess
helpers ignored when invoked alone), companion three passed (one subprocess helper),
MSRV full core suite plus the added 768/1536 churn test, both toolchains' clippy,
MSRV rustdoc with warnings denied, ten synthetic release-gate tests, 33 Python
document tests and Python/JavaScript full-u64/UTF-8 JSON contracts from an installed
source package. Packaged legacy/operations/filter/generation tests and external
application restart passed. Local core/companion ZIP extraction checks passed.

A local live-publisher smoke completed 12,000 ACK mutations in 194.448 seconds,
six restarts, one legacy backup, 46 generation publications, 23 SQ8 checks and one
generation backup. Its separate mapped reader completed 223 checks of 36,797
original records over 30.020 seconds and observed three generations. Both ran
against the local implementation, not registered candidate archives. These short
runs verify the harness and concurrency path; their 24-hour/100k gates are false.
