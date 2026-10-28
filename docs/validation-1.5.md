# 1.5 development validation (unreleased)

P0 session/batch CLI and P1 prepared-cache/block scans are implemented on the
development branch. P2 bounded mmap remains an experiment and is excluded.
These are local worktree builds; manifest versions stay at 1.4.0/0.1.0 until a new
candidate is prepared. This report does not approve a new registry release.

## Conditions and evidence identity

Apple M4 Pro, 48 GiB RAM, macOS 26.6.2 (25G83), 16 KiB pages. Rust 1.89.0
(29483883e, LLVM 20.1.7), aarch64-apple-darwin. CLI release uses thin LTO and
one codegen unit; companion/probe examples use their default release profile.
Each comparison keeps its binary, profile and backend fixed across its modes.

The large fixture has 50,000 synthetic 1,536-dimensional vectors and 19 segments.
SciFact uses 5,183 frozen 384-dimensional embeddings. Each has 20 calibration
queries and 100 separate held-out queries. SQ8 budgets are frozen at 64 and 16.
No new validation-query tuning or embedding/model execution occurred.

Filesystem cache was warm; fresh CLI/session processes were measured. Cold
filesystem cache, independent users and production applications were not measured.
No Vecnook builds, tests, models or soaks overlapped timed workloads; lightweight
driver/editor and other system activity were uncontrolled. A session handles one
request at a time; 1/2/4-reader measurements deliberately run independent processes.

Native /usr/bin/time -l peak RSS covers each entire engine process: startup,
source/cache validation, queries and exit. Preparation is another measured process.
Driver/time-wrapper memory is separate. Simultaneous group RSS is sampled every
100ms and excludes wrappers; it is not an exact high-water mark or physical RAM.
macOS PSS/private/shared attribution and global filesystem-cache memory are
unavailable here; absent metrics are not zero. MiB means 1,048,576 bytes.

## Actual CLI results

RPC time includes client JSON serialization, pipe I/O, server parsing/search/output
and client decoding after ready. Engine search time includes code scanning,
original reranking and result construction, excluding transport. CLI p95 uses
the nearest sample at round((N-1)*0.95), with all 100 query samples. Startup
is measured separately; sums of phase medians are not reported as CLI p95.

### 50,000 × 1,536

| Mode | CLI/RPC p95 ms, rounds 1 / 2 / 3 | Native peak RSS MiB, rounds 1 / 2 / 3 |
| --- | --- | --- |
| 1.4 fresh one-shot per query | 1005.032 / 1008.180 / 1008.003 | 78.812 / 78.859 / 78.844 |
| Retained resident SQ8 session | 16.827 / 17.203 / 16.826 | 78.391 / 78.328 / 78.406 |
| Prepared-cache session, 1 MiB workspace | 7.213 / 7.097 / 8.056 | 7.141 / 7.141 / 7.062 |

Cached engine-search p95: 6.526 / 6.415 / 6.873 ms.
Source open + cache validation + ready: 945.537 / 998.356 / 959.461 ms.
First query after ready: 6.989 / 7.129 / 7.149 ms.

| Cache preparation, including source validation | Rounds 1 / 2 / 3 |
| --- | --- |
| Entire command ms | 1578.968 / 1188.582 / 1174.605 |
| Entire process peak RSS MiB | 5.672 / 5.766 / 5.750 |

First cached one-shot command samples: 962.652 / 997.298 / 964.834 ms.
These three first-use samples are not a 100-query one-shot p95.

### SciFact 5,183 × 384

| Mode | CLI/RPC p95 ms, rounds 1 / 2 / 3 | Native peak RSS MiB, rounds 1 / 2 / 3 |
| --- | --- | --- |
| 1.4 fresh one-shot per query | 58.744 / 65.697 / 78.826 | 6.422 / 6.438 / 6.469 |
| Retained resident SQ8 session | 1.442 / 1.257 / 1.276 | 6.812 / 6.859 / 6.906 |
| Prepared-cache session, 1 MiB workspace | 0.638 / 0.587 / 0.665 | 3.922 / 4.844 / 4.891 |

Cached engine-search p95: 0.294 / 0.285 / 0.324 ms.
Source open + cache validation + ready: 55.155 / 54.579 / 54.377 ms.
First query after ready: 0.847 / 0.781 / 0.825 ms.

| Cache preparation, including source validation | Rounds 1 / 2 / 3 |
| --- | --- |
| Entire command ms | 85.558 / 82.281 / 84.561 |
| Entire process peak RSS MiB | 2.547 / 2.688 / 2.750 |

First cached one-shot command samples: 57.765 / 56.590 / 57.001 ms.
These three first-use samples are not a 100-query one-shot p95.

The large cache stores 76,800,000 code bytes and 800,000 parameter bytes;
complete DATA size is 78,600,455 bytes plus its 8-byte LEASE.
These are logical file sizes, not resident memory. The default 1 MiB read workspace
is row-aligned (1,047,600 bytes on this fixture). It is per search, not a bound on
the manifest index, output payloads, concurrency or total system RAM.

### Startup and buffer decision

Original source validation remains mandatory and scans all original records.
Cache opening also validates its full identity, CRC and parameter/code invariants.
A cache hit performs zero re-encodings, but its validation can take a similar
amount of time to the old encoding pass. First requests still wait roughly a
second on the large fixture; the main latency gain comes from session reuse.

The planned 4 MiB starting workspace reached 25.387ms engine p95 in one final
trial, missing the 25ms target. That trial and all earlier attempts are retained.
A rotated 4MiB/1MiB comparison kept the same binary, source, cache and queries:
the 1MiB option used about 7MiB process RSS versus 10–11MiB with 4MiB, with
similar query latency. The default was changed to 1MiB and the complete CLI
matrix was rerun. This is a workspace adjustment; the codec, candidates, quality
criterion and original-validation contract are unchanged.

The subsequent 1MiB trial also had a 33.725ms outlier. Buffer size alone did not
supply enough latency margin. The new cached scan now accumulates products in
i32: even 4096 full-range i8 products have absolute partial sums <=67,108,864,
well below i32::MAX. Converting the exact sum to i64 preserves the existing f64
distance formula. Exhaustive one-coordinate pairs, extreme 4096-coordinate
patterns and full response comparisons validate equality. No unsafe/ISA-specific
code, encoding change, candidate reduction or floating-point reassociation was added.

Final single-reader development gate (all three rounds, RSS<=40MiB, engine p95<=25ms, RPC p95<=50ms): **PASS**.
The target applies only to this specified input, K=10, candidate count and host;
it is not a service-level guarantee. Earlier 4MiB misses are not erased by tuning.

### Simultaneous readers

| Dataset / backend | Readers | RPC p95 range ms across readers/rounds | Highest sampled simultaneous engine RSS MiB |
| --- | --- | --- | --- |
| large / resident | 1 | 16.826–17.203 | 78.375 |
| large / resident | 2 | 16.862–18.288 | 157.469 |
| large / resident | 4 | 17.165–22.313 | 315.625 |
| large / cached | 1 | 7.097–8.056 | 7.125 |
| large / cached | 2 | 7.844–8.173 | 14.062 |
| large / cached | 4 | 8.609–9.114 | 27.984 |
| SciFact / resident | 1 | 1.257–1.442 | 6.859 |
| SciFact / resident | 2 | 1.264–1.340 | 13.641 |
| SciFact / resident | 4 | 1.257–1.409 | 27.234 |
| SciFact / cached | 1 | 0.587–0.665 | 0.000 |
| SciFact / cached | 2 | 0.618–0.657 | 0.000 |
| SciFact / cached | 4 | 1.310–3.439 | 19.562 |

These sums do not account for shared OS file-cache pages. More readers can
increase contention and latency; the single-reader target does not cover these groups.

## Correctness and regression

All 4,200 session responses per dataset (3 rounds × 2 modes × 1/2/4 readers
× 100 queries) matched the original 1.4 binary's IDs, order, f64 distance bits,
metadata, generation, sequence and search flags/counts. Original f32 bits and
signed zero remain covered by the generation/cache tests.

Rust 1.89 local checks: 173 core unit/integration tests plus 4 doctests, 3
companion platform tests, warning-free clippy/rustdoc and formatting. Three
collector-contract tests and three isolated mmap-proof tests passed. The full
Python tool suite has 13 tests; document/Python integration adds 33. All pass.
Cargo package/verify and the public-file check passed with 153 intended files.
Ignored subprocess entry points are test helpers, not disabled feature assertions.

The new tests cover strict bounded JSON/Unicode, full-u64 IDs, malformed/oversized
requests followed by valid requests, EOF/idle/kill lease release, pinned generations
across publication/GC, three metrics and dimension tails, candidate/filter boundaries,
exact resident/cache equivalence, source/cache corruption, truncation, valid-CRC
invalid parameters, short/interrupted writes and each cache publication failure.
A real child is killed after flushing a partial pending file: it never becomes
DATA, the source reopens, its lease releases, and explicit re-preparation succeeds.

Existing C3 file, full mmap, SIMD, SQ8 and supported legacy modes were rerun in
independent copies of the frozen fixtures: 15 large + 18 SciFact runs. Every
result checksum matches its frozen 1.4 counterpart. Generation modes have
recall@10=1.0 on these 100 queries; SciFact legacy HNSW remains 0.999.
After changing the new cache's default workspace, 12 resident SQ8 C3 runs were
repeated. The later integer-width optimization affects only the new cached scan,
whose final responses are compared above. Existing formats/default HNSW and
companion runtime source remain unchanged. This is local regression evidence, not new
cross-platform registry or full-24h candidate approval.

## P2 mmap decision: deferred

The isolated optional-companion proof uses a shared 32MiB mapping budget across
all 19 segments, with 8MiB windows aligned to the actual 16KiB host pages.
Copies and eviction hold one mutex; in-flight views remain charged. It keeps
the original file/pin alive and performs full replacement-reader validation.
Tests cover boundary/tail/overflow, multiple files, concurrent copies, empty
generations, corruption, dropping views and later generation reclamation.

All three modes below use the final 1MiB cached-code scan and ScalarKernel.
Only original-record I/O differs; this is not a comparison with old resident codes.

| Original reader | Engine search p95 ms, rounds 1 / 2 / 3 | Full peak RSS MiB, rounds 1 / 2 / 3 |
| --- | --- | --- |
| Positional file | 6.457 / 6.525 / 6.989 | 8.547 / 7.297 / 7.297 |
| Existing full mmap | 6.552 / 6.656 / 6.412 | 301.750 / 301.531 / 301.734 |
| 32MiB shared windows | 11.444 / 11.109 / 10.974 | 37.281 / 37.297 / 37.016 |

Every response matched 1.4 exactly. Window charges never exceeded 32MiB;
all views were unmapped and active bytes were zero after drop. Counters apply
to bounded windows only; full mmap's resident cost is measured by native RSS.

The earlier 4MiB-code-buffer proof had p95=24.008/24.142/25.299ms and missed
the latency target. The final windows pass the local absolute 128MiB/25ms targets, but file reads
remain faster and use less process RSS; window churn adds complexity without a consistent query benefit. Linux,
Windows 11 ARM64 NTFS and Windows Server x64 NTFS window lifecycle/alignment
proofs are absent. The experiment is excluded from the companion/public package.

## Reproduce and remaining release work

Evidence stays local under work/v1.5-evaluation, including:

- environment.json: host, exact compiler, runtime source and binary fingerprints.
- baseline-capacity and baseline-scifact: actual original one-shot samples.
- p1-capacity-opt-final and p1-scifact-opt-final: all final raw timings/results/RSS.
- buffer-pair and p1-capacity-codec-final: prior 4MiB results and paired tuning.
- p1-capacity-1m-final: the pre-integer-optimization 33.725ms outlier.
- c3-capacity-50000-1536, c3-scifact and c3-sq8-final: C3 logs and checksums.
- p2-capacity and p2-capacity-opt-final: all mmap attempts and decision evidence.
- validation: command logs; p2-mmap-probe: isolated experimental source.

Run tools/compare_cli_sessions.py --help for baseline/candidate/fixture/output,
three-round 1/2/4-reader collection and optional --buffer-bytes. Reuse a complete
baseline only when its binary/query/fixture/budget/count identity agrees.
Read [the generation guide](engine-generations.md) for JSONL and cache lifecycle.

Before any release: choose new package versions/RCs, run the new source's
Linux/macOS/Windows/MSRV CI and exact registry-consumer matrix, independent full
24h workloads, source-equivalence/publication gates and distribution verification.
No new registry publication, deployment, 24h approval or user-observation claim
is made by these local development checks. The published 1.4 artifacts are intact.
