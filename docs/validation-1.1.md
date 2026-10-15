# 1.1.0 validation

The release adds BM25/RRF document search, selective-filter exact search and
installed folder commands. Rust dependencies remain empty. Existing vector APIs,
JSON v1 fields, original payloads and snapshot/WAL/cache formats are preserved.
Memory/copy optimization and compound/range filters remain 1.2 scope.

Measurements ran on 2026-10-14, Apple M4 Pro, 48 GiB, arm64 macOS 26.6.2 with
Rust 1.89 release builds, thin LTO and one codegen unit. These are local workload
observations. Model setup and another operating system are outside these timings.

## Selective vector search

`filter_bench` builds 50,000 cosine vectors at 128 dimensions with 64 seeded
centers, corpus/query seeds 43/84, M=16, efConstruction=200 and efSearch=1024.
Each filter selects exactly 500 records. Both policies use the same graph,
queries and filters; the previous policy disables the new selective threshold.
An independent scalar f64 oracle checks returned distances, Top-10 boundaries,
duplicate IDs, eligibility and completeness. Each setting has ten warmups and
100 measured queries in each of three fresh sequential processes.

The table uses medians of runs 2, 3 and 4. Run 1 was conservatively excluded
because compilation overlapped process startup; raw output is retained.

| Eligibility path | Previous Auto p95, ms | 1.1 Auto p95, ms | Recall@10, both | 1.1 exact queries |
| --- | ---: | ---: | ---: | ---: |
| Explicit IDs | 18.934625 | 0.037792 | 100% | 100/100 |
| Metadata equality | 18.783833 | 0.028292 | 100% | 100/100 |
| Arbitrary predicate | 19.228833 | 0.119041 | 100% | 100/100 |

Mean distance computations fall from 50,096.44 to 500. Predicate queries still
evaluate all 50,000 active records; metadata equality compares 500, and explicit
IDs require no predicate evaluations. Unfiltered/forced-HNSW behavior is retained.
Auto selects this path for eligibility at most 1/8 of active records and at most
4096 candidates. The cap is a heuristic; the measured crossover here is 500/50k.

## Original-document relevance

Three fresh sequential evaluations use the same genuine local EmbeddingGemma
embeddings, 31 authored documents and 22 fixed source judgments (11 English,
11 Korean). Ten prose questions/answers are retained from `evaluate_demo.py`;
twelve identifier questions cover functions, errors and fixture product names.
The corpus includes 20 deliberately repetitive fictional I/O-code entries.
Judgments, vectors and results match across the three runs. This evaluates those
authored cases; it is not a general multilingual relevance estimate.

| Ranking | Hit@1 | Recall@3 | MRR@3 |
| --- | ---: | ---: | ---: |
| Vector | 95.45% | 100% | 0.977273 |
| BM25 | 77.27% | 77.27% | 0.772727 |
| RRF | 77.27% | 100% | 0.878788 |
| Automatic text selection | 95.45% | 100% | 0.977273 |

Every mode ranks all twelve identifier answers first. Vector and automatic
selection rank 9/10 prose answers first; unconditional RRF ranks 5/10 first.
This is why `docs-find` selects vectors for prose and hybrid for explicit
identifiers. The sample does not establish an aggregate relevance improvement
over the strong vector baseline. Keyword retrieval supplies a model-independent
exact-token path after import; Korean morphology is not implemented.

Model digest: `85462619ee721b466c5927d109d4cb765861907d5417b9109caebc4e614679f1`.
Embedding dimensions: 768. RRF offset/depth: 60/64.
Vectors SHA-256: `56de8de2845cd0e1dd246c51ef410c2750ad6aa703c8d7548b10d4bdc971ea94`.

## Installed first search

A fresh offline CLI installation from the checked package, sync of the project's
15 Markdown documentation files and a query outside the checkout took 11.879
seconds: installation 6.795, sync 4.940, query 0.144. Returned original text and
decimal string IDs were checked against source files, which remained unchanged.
The query returned `api.md`, `operations.md` and `app-integration.md`.

Rust, Python and the local embedding model were already installed. Model download
and toolchain setup are excluded, so this result supports the five-minute goal
for this prepared environment and folder. Reproduce the complete measured path:

```bash
python3 tools/check_first_search.py --crate target/package/vecnook-1.1.0-rc.1.crate --documents docs --output work/first-search-repeat
```

## Contracts and recovery

Current stable Rust and the 1.89 MSRV pass the full suite. Checks cover independent
BM25 counts and RRF ranks, Unicode/identifier tokens, deterministic ties, empty
queries, bounds, filter leakage, same-ID ordered batches, failed preflight,
deletion, WAL restart, compaction, named backups and logical restore. Existing
fault injection, process interruption, metrics, legacy binary fixtures and
application-worker tests continue to pass. The ignored crash child is invoked
by its parent process test, rather than run as an independent test.

The Python integration checks pass against both the checkout binary and a
fresh installed package, including folder updates/pruning, explicit/default
ranking selection, model mismatch, original text and running outside the checkout.
Independent Python/JavaScript consumers validate JSON v1 and full-u64 string IDs
for vector, keyword and hybrid commands. Package consumers also check old-file
upgrades, diagnosis, export/import/backup, app restart and prepared demo restart.
Formatting, Clippy with warnings denied, rustdoc, archive privacy and relative
documentation links pass. The source package contains no external Rust crates.

## 24-hour workload

**The registered candidate completed the full workload on 2026-10-15.**
The exact crates.io `1.1.0-rc.1` archive ran from 2026-10-14 at 10:34:50 KST
to 2026-10-15 at 10:34:50 KST. Its final receipt reports exit code zero,
`completed=true`, 86,401.111721625 elapsed seconds, 189,284 acknowledged
mutations, 89 restarts and 17 independent backup verifications. Both the
24-hour and 100k-mutation gates pass. The receipt and progress file agree;
stderr is empty and the archive/runtime hashes match the checked source.

All ten candidate tag-CI jobs passed, including eight exact-registry
installation/consumer receipts across Linux, macOS, Windows Server and Windows
11 ARM64/NTFS with both toolchains. Actual registry CLI/library installation,
the remote Git tag and docs.rs were also verified locally. Distribution receipts
and the completed [soak receipt](https://github.com/gay00ung/vecnook/releases/download/v1.1.0-rc.1/soak-receipt.json)
are attached to the [candidate release](https://github.com/gay00ung/vecnook/releases/tag/v1.1.0-rc.1).

- Registered crate SHA-256: `ec89f8b700fcf0ff89b83fa39b2c6233b612b20e5d2b2a1488f5e133412526e0`
- Registered runtime/test source SHA-256: `235385df2b0c705c165d950d93b33385f7c855b251b081a59adeceb00cf23702`

The extended workload mirrors acknowledged vector mutations into a typed
collection, validates lexical matches and independently fused ranks, checks
selective Auto, compacts, reopens, verifies independently named backups and
exports/imports original documents. Success requires exit code zero, final ACK
replay/results, `completed=true`, elapsed time at least 86400 seconds and at
least 100000 acknowledged mutations. A running or short test is not a duration
pass. The short 2000-mutation smoke run passed before starting the archived run.
An earlier local-archive workload was superseded and stopped; its retained files
are not used as stable-release evidence.

The stable promotion changes only the package version and documentation. The
release gate compares the packaged runtime, tests, tools and original build
settings to the registered candidate, allowing the own-package version change.
It also checks all distribution receipts and the completed soak receipt before
publication. Native Markdown commands emit UTF-8, including when Python's parent
output encoding differs, and preserve mixed CRLF/LF original documents.

Local measurements live under `work/v1.1-measurements`,
`work/v1.1-evaluation-{1,2,3}` and `work/v1.1-first-search`. Registered evidence
is under `work/v1.1-registry`, `work/v1.1-release-evidence` and
`work/v1.1-registry-soak-24h`; release progress is recorded in
`work/v1.1-release-state.json`.
