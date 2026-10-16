# 1.2 release validation

This release implements memory/update cost reductions and indexed Boolean,
numeric and date filtering. The core remains Rust std-only with Rust 1.89 support.
The registered candidate completed the full 24-hour QA on 2026-10-16. Eight
cross-platform registry distribution receipts also pass; the completed evidence
is linked below.

## Memory and update comparison

`tools/compare_memory.py` compiles the same public-API harness against the
checksum-verified registry 1.1.0 source and the candidate. Local measurements use
Rust 1.89.0 release builds on Apple Silicon, macOS 26.6.2. Each operation runs in
three fresh sequential processes against its version's independently prepared
database. The fixed fixture has 8,000 inserts, 32 coordinates and 4 KiB opaque
metadata per record, followed by 4,000 replacements and 2,000 deletions: 6,000
active IDs and 12,000 physical nodes. Updates/deletes measure 4,000 further
operations in 128-operation synced batches. Search performs 3,000 fixed queries
at K=10, forced HNSW ef=128. Ranking digests agree across versions and repetitions.

Medians from the repeated processes:

| Operation | 1.1 time (ms) | Candidate time (ms) | 1.1 peak RSS (MiB) | Candidate peak RSS (MiB) |
| --- | ---: | ---: | ---: | ---: |
| Checkpoint | 144.60 | 83.73 | 158.63 | 32.47 |
| Open | 117.34 | 67.50 | 105.66 | 32.42 |
| Compact | 677.97 | 661.87 | 160.47 | 34.73 |
| Update 4,000 | 750.47 | 763.25 | 128.89 | 54.75 |
| Delete 4,000 | 136.38 | 132.82 | 105.91 | 32.55 |
| Search 3,000 | 329.86 | 337.50 | 105.52 | 32.80 |

Search p95 is 0.1310 ms in 1.1 and 0.1331 ms in the candidate. The small search
and update slowdowns are reported as measured; this change does not establish a
query-throughput improvement. Checkpoint peak RSS falls about 80%, and compaction
peak RSS about 78%, in this metadata-heavy churn fixture.

RSS comes from `/usr/bin/time -l` for the whole measured process, including open
and verification; it is not an allocation count or per-phase peak. Each version
writes its own snapshot, so the comparison includes inactive payload reclamation
as well as streamed I/O. Filesystem caches are warm and the shared local machine
is not CPU-isolated. These numbers do not predict every dimension, payload size,
dataset or OS. Raw repeated output and machine-readable results are retained as
release evidence; the harness and runner are included in the crate.

```bash
python3 tools/compare_memory.py \
  --baseline /path/to/verified/vecnook-1.1.0 \
  --candidate . --output work/memory-comparison
```

Search visited/eligibility scratch uses one u64 per 64 physical nodes; it remains
local to each query. Snapshots/caches use 64 KiB streaming buffers. Compaction
persists active records before moving payloads into a rebuilt graph. Deleted and
superseded payload strings are released while coordinates/links remain traversal
paths until compaction. Repeated IDs in a batch update document postings once to
the final state. RRF clones only returned documents after ranking borrowed candidates.

## Correctness and compatibility

Independent document scans check 140 seeded Boolean/range expressions across
exact vector, keyword and hybrid modes, plus explicit HNSW eligibility. Cases
include missing attributes, negation, empty AND/OR, equal/exclusive endpoints,
legacy source/tag intersections, repeated-ID batches, deletion, compaction,
reopen, backup and logical import. Parser checks cover precedence, JSON/Unicode
escapes, invalid scalars and size/depth/node budgets.

Existing storage tests remain in force. New checks cover partial writes across
the streaming-buffer boundary, short/interrupted snapshot/cache writes, moved
live payloads, replay byte-budget equality and actual 1.1-generated files.
The old VDOC1 bytes are checked exactly; typed VDOC2 payloads are bounded and
validated. See [reader migration](compound-filters.md#payload-and-reader-compatibility).

Python integration exercises opt-in file metadata, inferred repeat sync,
metadata-only changes, explicit opt-out and every ranking mode. Keyword filtering
also runs with a client that rejects embedding/model calls. Synthetic embeddings
prove transport and persistence; they are not new semantic search-quality evidence.

The existing real EmbeddingGemma evaluation was also rerun with the same model
digest, vectors, 31 authored documents and 22 English/Korean questions. All 88
vector/keyword/hybrid/auto responses equal the actual registry-installed 1.1.0
binary's responses on identical stored vectors, including scores and diagnostics.
Auto retains 21/22 Hit@1 and 22/22 Recall@3; this establishes parity for the fixed
fixture, not improvement on arbitrary documents. Separately, 320-record fixtures
in all three metrics produce byte-identical snapshots/graph caches and identical
HNSW reports between the two binaries before any tombstone reclamation.

## Completed registry QA and promotion

The exact crates.io `1.2.0-rc.1` archive ran from 2026-10-15 at 13:53:12 KST to
2026-10-16 at 13:53:12 KST. Its final receipt reports exit code zero,
`completed=true`, 86,401.130573 elapsed seconds, 189,283 acknowledged mutations,
89 restarts and 17 independent backup verifications. Both duration and mutation
gates pass. The receipt, progress file and final stdout agree; stderr is empty.
The archive, executed workload and current runtime/test/build fingerprints match.
See the [completed receipt](https://github.com/gay00ung/vecnook/releases/download/v1.2.0-rc.1/soak-receipt.json).

The workload derives changing numeric/date attributes from an independent ACK
model, checks Boolean eligibility across all three ranking modes, and repeats
compaction, restart, backup and logical import. Failed or interrupted runs cannot
supply successful release evidence.

All ten [candidate tag-CI jobs passed](https://github.com/gay00ung/vecnook/actions/runs/38683872221),
including eight fresh-cache exact-registry CLI/library consumers across Linux,
macOS, Windows Server and Windows 11 ARM64 with Rust 1.89 and current stable.
Both Windows 11 receipts record desktop product type 1 and NTFS. The individual
`distribution-*.json` receipts are attached to the
[candidate release](https://github.com/gay00ung/vecnook/releases/tag/v1.2.0-rc.1).

Stable promotion changes the package version and Markdown documentation only.
[The release gate](release-readiness.md) checks all receipts and requires identical
normalized runtime, tests, tools and build settings in the prepared stable archive;
publication also requires the clean stable commit's CI to pass.
