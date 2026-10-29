# Current SciFact search benchmark

Measured on **2026-10-29** with **Vecnook 1.5.0-rc.1**, built from `main`
at [5eab06d](https://github.com/gay00ung/vecnook/commit/5eab06db209ec04b1c597a36514a46484a98ba16).
This measurement records that exact source candidate; it does not approve a stable release.

The sweep measures native in-memory HNSW search against exact cosine Top-10 on
5,183 SciFact vectors, 768 dimensions, and 300 independent test queries. It uses
the same frozen EmbeddingGemma outputs as the [historical text evaluation](text-benchmarks.md).
Recall describes nearest-neighbor agreement, rather than relevance against human labels.

## Results

Each setting ran in **three fresh sequential processes**. The table reports the
median of each process-level metric; p95 is not pooled across the three runs.
The minimum–maximum column and chart whiskers show observed process-level p95
variation, rather than confidence intervals. All 18 runs returned ten results for
every query, and recall was identical across the three runs at each setting.

| efSearch | Recall@10 | Median p50 ms | Median p95 ms | p95 min–max ms | Median sequential QPS |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 16 | 96.5000% | 0.192000 | 0.305750 | 0.220416–0.389709 | 4891.23 |
| 32 | 98.8667% | 0.288083 | 0.373458 | 0.363542–0.688459 | 3400.48 |
| 64 | 99.7667% | 0.478917 | 0.615750 | 0.608208–0.792416 | 2054.15 |
| 128 | 99.9333% | 0.769000 | 0.976000 | 0.896625–1.233750 | 1289.31 |
| 256 | 100.0000% | 1.239333 | 1.626375 | 1.399041–1.635584 | 795.16 |
| 512 | 100.0000% | 1.817500 | 2.267959 | 2.085417–2.410000 | 541.40 |

![Search latency and Recall@10 measured with Vecnook 1.5.0-rc.1](https://raw.githubusercontent.com/gay00ung/vecnook/main/docs/media/search-benchmark.png)

[All 18 raw results, environment, hashes, and aggregation](https://github.com/gay00ung/vecnook/blob/main/docs/benchmarks/scifact-2026-10-29.json)
include the unedited native CLI output from every child process. No runs were discarded.

## Measurement scope

- Hardware: Apple M4 Pro, 48 GiB RAM, arm64.
- OS: macOS 26.6.2 (25G83). Compiler: `rustc 1.96.0 (ac68faa20 2026-05-25)`.
- Build: `cargo build --locked --offline --release --bin vecnook`; thin LTO, one codegen unit.
- Index: M=16, efConstruction=200, seed=42, original `f32` coordinates, cosine distances accumulated in `f64`, K=10.
- Timing: ten warmup queries, followed by 300 sequential queries per process. Includes result construction. p95 uses the nearest-rank percentile.
- Reference: the current binary's exact cosine search on the same inputs; exact-oracle work is outside timed HNSW queries.
- Excluded: embedding generation, file loading, graph construction, process startup, persistence, filters, and the optional generation/SQ8/SIMD/mmap paths.

The driver ran one benchmark child at a time. Settings rotated between rounds:
`16,32,64,128,256,512`, then `64,128,256,512,16,32`, then
`256,512,16,32,64,128`. Each process builds a fresh index. Filesystem cache was
not flushed. No build or plotting jobs from this measurement overlapped the
benchmark; other host activity was uncontrolled.

The historical 0.3 measurements used a different compiler and run session.
These new numbers alone do not establish a speedup or regression against 0.3.
This small scientific-text dataset also does not establish performance for other
corpora, production concurrency, or maximum configured scale.

## Input and binary identity

The frozen input hashes match the historical evaluation's published hashes.
Dataset text, model weights, and vector files are not redistributed here.
The model identity and preprocessing are documented in the
[original input method](text-benchmarks.md#inputs-and-reproduction).

| Artifact | SHA-256 |
| --- | --- |
| Base fvecs | `4995c03f6910c437ae5a60c8c62392af28eeee33c486e978de6b3216b11783fb` |
| Query fvecs | `84735c492ff0e4c0e951e7f1d36cc728231192a497ccb2d93a2a0761edf6678e` |
| Measured release binary | `7d4099c0fe112954c102ad57f7a3263b5ca5496a23ad80c535911688a1347de5` |

The driver verified the binary hash before and after all runs, checked the
version and dataset dimensions in every result, and rejected incomplete query results.

## Reproduce

From a checkout with Rust and Python 3, using the frozen files matching the
hashes above:

```bash
cargo build --locked --offline --release --bin vecnook
python3 tools/benchmark_readme.py work/scifact/base.fvecs work/scifact/query.fvecs \
  --output work/readme-scifact-rerun.json
```

The driver requires macOS for the recorded hardware metadata. Choose a fresh
output path; it preserves evidence rather than overwriting an earlier run.
To generate your own SciFact inputs, use `tools/prepare_embeddings.py` as described
in the original input method. Different model/backend versions may change vectors;
retain the generated manifest and report any changed hashes.

To recreate the README chart from the committed evidence, run
`python3 tools/render_readme_benchmark.py` with Matplotlib installed in your
rendering environment. Matplotlib is not a Vecnook dependency.
