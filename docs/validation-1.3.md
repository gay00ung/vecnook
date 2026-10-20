# 1.3 candidate validation

The 1.3 candidate adds document-client and owning-worker features while retaining
the default tokenization, vector math and persisted snapshot/WAL/cache formats.
Stable promotion requires the exact registered RC's completed 24-hour workload,
100k acknowledged mutations and registry distribution receipts, including actual
Windows 11 NTFS. These gates are pending until successful receipts are recorded.

## Local correctness

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

## Authored relevance cases

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
`302d2c09ddeb79b3a980f3bf28f30e0286a4c1dbb2ebb7c10066de6ed99c3187`.
Expanded judgments SHA-256 is
`aec9e9bae4df926c8775ebd92486a05504454e2f8745560d662b10517917dd11`;
the f32 embedding input/output vector set SHA-256 is
`7c94803af05be020950c0867a7da584fa028ee2071594afd7c636be9d773cb1d`.
Raw authored results retain per-question source ranks and split labels.

## Local operating-range comparison

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
| 1,000 | 1.3 candidate | 0.185 ms | 0.225 ms | 22.47–25.22 MiB |
| 10,000 | 1.2 | 0.171 ms | 0.184 ms | 122.80–128.17 MiB |
| 10,000 | 1.3 candidate | 0.166 ms | 0.181 ms | 137.39–139.20 MiB |

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
