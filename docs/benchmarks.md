# Benchmark measurements

Measurements recorded on 2026-10-03: Apple M4 Pro, 48 GiB RAM, macOS 26.6.2 arm64, rustc 1.96.0 (`ac68faa20`), release profile with thin LTO and one codegen unit. No concurrent search clients were used.

## SIFT small

The corpus contains 10,000 128-dimensional vectors and 100 queries. Files were obtained from the [SIFT small directory in DataStax jvector](https://github.com/datastax/jvector/tree/e31aa59510bdd58b79d7424543d2f8763f306779/siftsmall), pinned to commit `e31aa59510bdd58b79d7424543d2f8763f306779`. Dataset files are not redistributed in this repository or Cargo package.

Parameters: squared L2, K=10, M=16, efConstruction=200, graph seed=42. Each row is one run. Ten warmup queries are excluded from timings. Exact search supplies the Top-10 reference for the CLI evaluator. A separate check against the supplied `siftsmall_groundtruth.ivecs` returned the same 100% exact recall, with no boundary-tie differences; HNSW recall also matched the figures below.

| efSearch | Recall@10 | HNSW p95 (ms) | HNSW sequential QPS | Exact sequential QPS | Build (s) |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 32 | 99.70% | 0.050791 | 25,910.07 | 2,246.88 | 1.874285 |
| 128 | 100.00% | 0.146166 | 9,367.06 | 1,912.75 | 1.861407 |
| 512 | 100.00% | 0.295791 | 4,173.77 | 2,169.61 | 1.794751 |

All 100 queries returned K results. Mean HNSW distance computations were 500.86, 1,162.71, and 2,498.17 respectively, versus 10,000 for exact search. The graph had 269,837 directed edges and four layers. Coordinate payload was 5,120,000 bytes; this is not process RSS.

These timings measure direct `search_exact` and `search_hnsw` calls, including result construction. They exclude file loading, graph construction, persistence, CLI process startup, networking, and concurrent throughput. They also exclude predicate/allow-list construction in `search` and `search_filtered`. QPS is queries divided by the sum of measured search-call durations, not a load-tested service capacity. p95 uses the nearest-rank percentile. Timing varies with CPU and filesystem state; the rows are not confidence intervals.

To reproduce the CLI evaluation from a checkout:

```bash
mkdir -p work/datasets
curl --fail --location https://raw.githubusercontent.com/datastax/jvector/e31aa59510bdd58b79d7424543d2f8763f306779/siftsmall/siftsmall_base.fvecs --output work/datasets/siftsmall_base.fvecs
curl --fail --location https://raw.githubusercontent.com/datastax/jvector/e31aa59510bdd58b79d7424543d2f8763f306779/siftsmall/siftsmall_query.fvecs --output work/datasets/siftsmall_query.fvecs
cargo build --offline --release
./target/release/vecnook bench-file work/datasets/siftsmall_base.fvecs work/datasets/siftsmall_query.fvecs 10000 100 32 l2
./target/release/vecnook bench-file work/datasets/siftsmall_base.fvecs work/datasets/siftsmall_query.fvecs 10000 100 128 l2
./target/release/vecnook bench-file work/datasets/siftsmall_base.fvecs work/datasets/siftsmall_query.fvecs 10000 100 512 l2
```

The evaluator reads up to the requested corpus/query limits. A shorter nonempty file uses its actual count. It rejects partial records, non-finite coordinates, mixed dimensions, incompatible query dimensions, and invalid resource parameters. Extra records after a requested prefix are not validated.

SHA-256 of the downloaded files:

```text
1e90414a3254361aba48a0d58e461f7661fb135dabb3da985290e94de8f60fe0  siftsmall_base.fvecs
095af38bda2741be721447ba97e9286fa918ece946be92a9ae133276eddc1a10  siftsmall_query.fvecs
b0717bf276111a166f14dcc2653fd04465b2afd6d34c72be641b622cf99f95ae  siftsmall_groundtruth.ivecs
```

## Reopening and the previous implementation

The checkpoint cache removes full HNSW reconstruction for unchanged checkpointed nodes. A comparison used separate persistent directories containing the same 10,000 SIFT records, M=16, efConstruction=200, seed=42, and no pending WAL writes. The baseline was the 0.1 release binary from commit `66b0dde`, using a v1 snapshot. 0.2 upgraded an identical snapshot by checkpointing, producing a validated graph cache.

Each sample launched the CLI `stats` command in a new process, opened and inspected the database, then exited. One untimed invocation warmed each directory, followed by five timed invocations. The filesystem cache was not flushed: these are process reopen measurements with warm filesystem data, not cold-disk startup. Process creation and stats output are included. The final timing run was sequential, without another benchmark running alongside it.

| Version / open path | Median (ms) | Five samples (ms) |
| --- | ---: | --- |
| 0.1 / rebuild graph | 3215.01 | 2598.79, 3215.01, 3554.69, 3170.25, 3247.60 |
| 0.2 / validated cache | 36.33 | 40.74, 40.87, 36.33, 36.17, 36.08 |

These medians describe this checkpointed dataset only. A database with a large pending WAL still needs to apply changes and construct new graph nodes, and a cache fallback restores the rebuild cost.

## Limits of the evidence

SIFT descriptors are not modern text embeddings, and 100 queries are a small evaluation set. A synthetic uniform fixture with 10,000 vectors, 128 dimensions, 100 independent queries, seed=42, and efSearch=128 produced only 88.60% Recall@10 in both 0.1 and 0.2. Its identical edge count and mean distance computations provide a regression check, not proof of general accuracy. Full result counts do not guarantee nearest-neighbor quality.

The measurements establish neither superiority over Qdrant, pgvector, USearch, nor any other engine. There is no same-machine third-party benchmark here. Million-vector workloads, maximum configured resource use, peak RSS, storage latency, power-loss recovery, and sustained concurrent application workloads have not been characterized. Evaluate representative queries and filters against exact search before choosing parameters.
