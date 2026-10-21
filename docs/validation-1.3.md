# 1.3 validation

The 1.3 release adds document-client and owning-worker features while retaining
the default tokenization, vector math and persisted snapshot/WAL/cache formats.
The exact registered RC2 completed its 24-hour workload on 2026-10-21. Eight CI
registry consumers plus a local Mac consumer passed, including actual Windows 11
ARM64 on NTFS. Stable promotion changes only the version and documentation;
normalized runtime, tests, tools and build settings match RC2.

## Completed registered-candidate QA

The checked `1.3.0-rc.2` archive SHA-256 is
`c3a82222911ccd5c867c081e8f2e253811b42253498e2d9a09c7bb0b7ef6018b`.
Its normalized runtime/test/tool/build fingerprint is
`0163ef50087d32b05f1669dcc4eb8e53eef9bf58a7f3452ec33c12024a3243ff`.
The [candidate release](https://github.com/gay00ung/vecnook/releases/tag/v1.3.0-rc.2)
contains the exact archive, distribution receipts, native archives, Python wheel
and measurement reports.

The independently timed mixed workload completed 86,400.650580 actual workload
seconds, 189,268 acknowledged mutations, 89 restarts and 17 independent backup
verifications. Vector, BM25, RRF, selective Auto, compound ranges and streamed
compaction checks passed; acknowledged writes were replayed independently. The
[completed receipt](https://github.com/gay00ung/vecnook/releases/download/v1.3.0-rc.2/soak-receipt.json)
and progress agree, with exit code 0 and both duration/mutation gates true.
`tools/check_release.py` passed with the registered archive and all nine receipts.
This covers the recorded environment and workload, not arbitrary power loss or
all applications.

Tag CI tested Rust 1.89 and stable on Linux, macOS, Windows Server and Windows 11
Enterprise ARM64. Both Windows 11 receipts record desktop product type 1 and NTFS;
Server receipts are separate. Each registry consumer used a fresh Cargo cache,
compared CLI/library archive checksums with an independent download, and exercised
application restart, legacy upgrade, JSON/document/filter contracts and backups.
Native ZIP extraction/demo/restart, the remote Git tag installation, isolated wheel
and Node client, and versioned docs.rs also passed for RC2. Public asset digests
were checked against GitHub's recorded SHA-256 values.

The first RC2 workload stopped making progress during a host file-open stall and
was interrupted. Its cause remains unresolved; it is not attributed to a runtime
defect. The replacement used a separate accessible directory and the identical
registry archive, with no source changes. Only the replacement's complete duration
counts above. RC1 and the interrupted RC2 data/logs are preserved and contribute
no elapsed time to approval.

## RC2 ranking and memory corrections

RC1 counted added Korean grams as extra document length. In a bilingual corpus,
that changed BM25 normalization even when an English query's terms and document
frequencies were unchanged. RC2 counts original lexical positions for both
analyzers. A query without Hangul syllables therefore retains the Words score
and order. Repeated grams within one word remain deduplicated. Regression tests
cover hand-calculated Korean BM25 counts, English/Unicode/identifier scores,
filters, replacements, deletion and reopen; the English-score test fails on RC1.

The 230-case evaluation was repeated with the same model digest, judgments and
identical f32 vector-set hash recorded below. All default vector/keyword/hybrid/
auto parsed JSON still matched 1.2. Optional Korean keyword results changed as
follows:

| Cases | RC1 Hit@1 | RC2 Hit@1 | Default Words Hit@1 |
| --- | --- | --- | --- |
| Korean prose | 92/100 | 93/100 | 65/100 |
| English prose | 32/50 | 35/50 | 35/50 |
| Exact identifiers | 50/50 | 50/50 | 50/50 |

Across 200 answerable cases, RC2 Korean keyword Hit@1 is 89%, MRR@10 0.9271,
nDCG@10 0.9394 and Source Recall@10 97.5%. All 30 unrelated keyword queries
remain empty. These reused cases now serve as regression evidence, not a blind
holdout or independent user study. The analyzer remains opt-in and partial Korean
matches can be unrelated. The evaluated RC2 CLI SHA-256 is
`b488188aa977b3ec285ca9a2ba367106426267b3fa36a29dfeb0388c24786b8d`.

Memory investigation separated live allocations from process RSS. At the same
1,000-document cached-open point, 1.2 and RC1 each retained 12,718 allocations
(9,640,864 and 9,640,848 bytes respectively). This snapshot does not show an extra
retained copy of vectors or documents in RC1. RSS differences across repeated
build/open/churn phases are consistent with allocator-region occupancy and
retention, rather than a larger live payload set. This does not assign every
observed RSS byte to one source change. Apple's [heap-analysis guidance](https://developer.apple.com/videos/play/wwdc2024/10173/)
explains why allocation lifetime, fragmentation and process memory measures must
be examined separately.

RC2 bulk-constructs the disposable keyword trees on open and analyzer changes,
borrows already-lowercase ASCII tokens, and avoids temporary frequency maps for
incremental Words updates. Invariant checks construct only the selected analyzer.
The [Rust 1.89 map constructor](https://github.com/rust-lang/rust/blob/1.89.0/library/alloc/src/collections/btree/map.rs#L2191-L2205)
uses bulk construction for collected entries. A separate externally paused
10,000-document cached-open snapshot retained 116,975 allocations / 92,312,240
bytes in RC2 versus 123,770 / 93,802,352 in RC1: 6,795 fewer allocations and
1,490,112 fewer live bytes. These snapshots are profiling observations, not
completed soak runs.

Three fresh sequential runs per version/count repeated the unchanged 768D
operating-range runner, rotating the 1.2/RC1/RC2 order. No builds, model work or
other QA overlapped timed processes. All input hashes, independent exact/recall,
eligibility, deleted-ID, duplicate-ID and result-count assertions passed.

| Documents | Version | Peak RSS median | Peak RSS range across all three runs |
| --- | --- | --- | --- |
| 1,000 | 1.2 | 20.56 MiB | 20.41–20.95 MiB |
| 1,000 | RC1 | 22.94 MiB | 22.20–23.05 MiB |
| 1,000 | RC2 | 20.95 MiB | 20.44–21.22 MiB |
| 10,000 | 1.2 | 124.19 MiB | 115.52–126.17 MiB |
| 10,000 | RC1 | 136.39 MiB | 135.67–141.00 MiB |
| 10,000 | RC2 | 118.31 MiB | 113.75–150.58 MiB |

RC2 reduced the sampled RSS median versus RC1 by 8.7% / 13.3% at 1k / 10k.
The 150.58 MiB RC2 outlier exceeds RC1's observed maximum, so this is not a uniform
peak reduction or a production memory bound. The consistent live-allocation
reduction and all peak samples are retained alongside the median.

The short 100-query post-churn p95 was 0.1965 / 0.2090 / 0.2293 ms for 1.2 /
RC1 / RC2, triggering the 10% investigation threshold against 1.2. A separate
sustained check used a post-churn snapshot AFTER compaction, with 9,000 physical
nodes and 9,000 active records, 200 warmups and
20,000 timed queries per process, with three rotated repeats. Median p95 was
0.1936 / 0.1786 / 0.2046 ms respectively; RC2 was 5.7% above 1.2. Every run's
result-ID checksum matched. These measurements include original-document decoding
and exclude embeddings and queue wait; they establish neither a speedup nor an
application SLA.

A second sustained probe captured the snapshot/WAL/graph BEFORE compaction,
with 11,000 physical nodes and 9,000 active records. It used the same 200 warmups,
20,000 timed queries per process and three rotated repeats without overlapping
builds, model work or other QA. Median p95 was 0.198375 / 0.195500 / 0.201375 ms
for 1.2 / RC1 / RC2, respectively (RC2 1.5% above 1.2). Every run had the same
result-ID checksum and 274.97 mean distance computations. See the
[full pre-compaction measurements](https://github.com/gay00ung/vecnook/releases/download/v1.3.0-rc.2/qa-warm-precompact.json).
The different physical graph and sample count matter when comparing this probe
with the short churn sample; neither sustained probe establishes an application SLA.

RC1's interrupted long QA is preserved at 720 seconds and 51,230 ACK mutations.
It does not approve RC2. Only the separate completed RC2 workload described above
satisfies the duration gate; no partial or earlier run contributes elapsed time.

## Shared correctness checks

New checks cover read-only inspection of an incomplete WAL without byte changes,
unchanged/metadata-only/chunk-level embedding reuse, empty and explained output,
environment errors, Korean analyzer mutation/reopen behavior, all worker retrieval
modes, status, stale writes, compaction and independent backups. Context assertions
reject excluded documents even when they are adjacent to an eligible result.
JSONL checks retain full-u64 string IDs, original text and signed-zero f32 vectors,
and reject invalid/duplicate records before claiming a destination. Python custom
embedding and native error contracts run against the actual local CLI.

The native archive runner extracts each ZIP and verifies its version, offline
demo, restart and embedded client help. Archive build host/target and checksums
are recorded separately from registry distribution receipts.

A separately built Python wheel was installed into an isolated external site and
used from a clean virtual environment. Caller-owned embeddings, full-u64 IDs,
original Korean text, vector/keyword search, reranker payload preservation and
JSONL restore passed. The asynchronous Node bridge returned the original string
IDs and text. The wheel includes the project's complete MIT notice.

## RC1 authored relevance cases

`tools/evaluate_relevance.py` predeclares 70 authored documents and 230 questions:
100 Korean prose, 50 English prose, 50 exact identifiers and 30 unrelated queries.
The prose uses templates over 20 authored bilingual topics; identifier documents
use fictional error codes. Labels are authored from the document topics, not
independent human judgments. Every fifth question is held out from the development
subset; no query-policy tuning is performed using validation output.

The final local run used genuine local EmbeddingGemma embeddings and 768
dimensions, with model digest
`85462619ee721b466c5927d109d4cb765861907d5417b9109caebc4e614679f1`.
Default vector/keyword/hybrid/auto parsed native JSON exactly matched the fresh
1.2 source binary on identical stored inputs. Results among 200 answerable cases:

| Mode | Hit@1 | MRR@10 | nDCG@10 | Source Recall@10 |
| --- | --- | --- | --- | --- |
| Vector / Auto | 100% | 1.000 | 1.000 | 100% |
| Default keyword | 75% | 0.788 | 0.804 | 85% |
| Hybrid | 86% | 0.919 | 0.939 | 100% |
| Optional Korean keyword | 87% | 0.913 | 0.929 | 97.5% |

Korean Hit@1 increased from 65/100 to 92/100 and identifier Hit@1 remained 50/50.
English Hit@1 decreased from 35/50 to 32/50 in this bilingual corpus: additional
document tokens change BM25 length normalization even for an English query.
The policy remains opt-in. Among the 40 held-out answerable questions, keyword
Hit@1 changed from 77.5% to 92.5%. This authored/template fixture does not establish
general or production multilingual quality, and no policy was tuned on its output.

The existing 31-document/22-question evaluation also passed default-response
equivalence. Vector/auto Hit@1 was 21/22, keyword and hybrid 17/22; vector, hybrid
and auto source Recall@3 was 22/22. A hybrid mode is not automatically better.

All 30 unrelated queries still returned vector/hybrid candidates; keyword returned
none. Returning K candidates does not establish that the corpus answers a question.
The optional distance bound remains application-calibrated, and `complete` remains
a count observation. Default retrieval and tokenization are unchanged.

The evaluated CLI SHA-256 is
`a4a4d960b2ef40a786af7bb5072b2da1ef7496d6b30b175882fba0b5786f437b`.
Expanded judgments SHA-256 is
`aec9e9bae4df926c8775ebd92486a05504454e2f8745560d662b10517917dd11`;
the f32 embedding input/output vector set SHA-256 is
`7c94803af05be020950c0867a7da584fa028ee2071594afd7c636be9d773cb1d`.
Raw authored results retain per-question source ranks and split labels.

## RC1 operating-range comparison

Both native benchmark binaries used Rust 1.89.0 on the same macOS ARM64 host.
Three fresh sequential runs per version/count used identical hashed synthetic
768-dimensional clustered vectors, 100 queries, original authored repeated text,
K=10 and an independently checked Recall@10 target of at least 99%. HNSW selected
ef=64 in these cases. Mutation/restart/compaction phases, selective filters and
1/4/8 concurrent submitting clients were included. Model work and builds did not
overlap timed benchmark processes. The 1.2 baseline is commit `0c1dbe4`.

| Documents | Version | Median engine p95, initial | Median engine p95, after churn | Full-process peak RSS, three runs |
| --- | --- | --- | --- | --- |
| 1,000 | 1.2 | 0.186 ms | 0.226 ms | 18.67–21.03 MiB |
| 1,000 | 1.3 RC1 | 0.185 ms | 0.225 ms | 22.47–25.22 MiB |
| 10,000 | 1.2 | 0.171 ms | 0.184 ms | 122.80–128.17 MiB |
| 10,000 | 1.3 RC1 | 0.166 ms | 0.181 ms | 137.39–139.20 MiB |

The median engine measurements did not exceed the 10% investigation threshold.
Individual runs had latency outliers, so these results do not establish a speedup
or an application SLA. All exact-reference, eligibility, deleted-ID, duplicate-ID
and result-count assertions passed. Full-process memory was higher in the candidate.
A separate matched-toolchain process opening the same 10,000-document snapshot
used 53.59–53.62 MB in 1.2 and 53.61–53.67 MB in the candidate. The full-workload
increase appears after repeated phases; this evidence does not identify its cause
or justify a general memory-improvement claim. The earlier exploratory runs are
retained separately rather than replacing the final three runs.

For 10,000 documents, median cached-open time was 170/175 ms (1.2/candidate), WAL
recovery 1.219/1.242 s and compaction 8.123/8.149 s. Candidate worker roundtrip
p95 was 0.167/0.623/1.212 ms with 1/4/8 submitting clients. This includes queue,
engine and response time; pure queue wait is not isolated. Separate 20-process
`--version` samples had medians 2.82/1.96 ms, including process launch/CLI dispatch.
Ten warm local single-query embedding requests had a 62.17 ms median roundtrip;
model download and a cold model load were not measured. These boundaries prevent
attributing embedding or process cost to engine search. No competitor performance
ranking is claimed by this validation.

## External observations

The owner confirmed no external participants/apps are available for the proposed
five-person first-use study. External first-use completion and repeat adoption
remain unmeasured; deterministic integration tests are technical evidence only.
This does not satisfy that product research goal and is not presented as user
acceptance. Existing release policy treats adoption observations separately from
the technical publication gate.
