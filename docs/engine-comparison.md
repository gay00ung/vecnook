# Local engine comparison

Measured on **2026-10-29** using Vecnook **1.5.0-rc.1** built from `main`
at [30f5dee](https://github.com/gay00ung/vecnook/commit/30f5dee3e33420fcaa6940a56e954408b305c842),
USearch **2.26.6**, and Chroma **1.5.9**.

All three receive identical precomputed SciFact vectors and queries, with the same
cosine metric and HNSW construction/search settings. The comparison measures warm
single-query calls through each named local API. Chroma is a vector database;
USearch is an indexing library. Vecnook is measured through its in-memory index.
The API and storage scopes differ, so these numbers describe these call paths,
rather than an isolated ranking of database cores.

## Results

Each target ran in three fresh sequential processes. Values are medians of the
three process-level metrics. The p95 range is observed min–max, rather than a
confidence interval; query samples are not pooled between processes.

| Engine/version | Measured call path | Recall@10 | Median p50 ms | Median p95 ms | p95 min–max ms |
| --- | --- | ---: | ---: | ---: | ---: |
| Vecnook 1.5.0-rc.1 | Rust in-memory index API | 99.9333% | 0.801042 | 1.309875 | 1.037792–2.062000 |
| USearch 2.26.6 | Python binding to native index | 99.8333% | 0.691958 | 0.924916 | 0.806584–1.197167 |
| Chroma 1.5.9 | Python local persistent DB API | 99.9000% | 1.605083 | 2.842417 | 2.160083–3.095541 |

![Fixed-configuration comparison of Vecnook, USearch, and Chroma local query-call latency and recall](https://raw.githubusercontent.com/gay00ung/vecnook/main/docs/media/engine-comparison.png)

USearch had the lowest median p95 query-call latency in this session. All three
returned ten results for every query and exceeded 99% Recall@10 in every run.
Chroma includes validation, local database query handling, and a Python response;
its result cannot be read as the cost of its HNSW kernel alone. The chart uses a
logarithmic latency axis, identified in the axis label, and shows observed ranges.

## Shared conditions

- Hardware: Apple M4 Pro, 48 GiB RAM, arm64 macOS 26.6.2 (25G83).
- Data: the frozen BEIR SciFact corpus, 5,183 original 768-dimensional `f32` vectors and 300 independent test queries.
- Metric/results: cosine, K=10, original vectors without scalar/product quantization; compare IDs to an independent exact reference.
- HNSW: M/connectivity/max_neighbors=16, efConstruction/expansion_add=200, efSearch/expansion_search=128.
- Execution: one HNSW worker, one query per call, ten warmup queries and 300 timed queries per fresh process.
- Timing: nearest-rank p50 and p95, including each API's result construction; recall and ID checks occur outside the timer.
- Exclusions: embeddings, input loading, graph construction/ingestion, process startup, network, concurrent clients, filters, and memory comparisons.

The search setting was fixed at 128 **before** the comparison. There was no
per-engine candidate-pool tuning or configuration selection from measured results.
Vecnook uses seed=42; Chroma and USearch do not expose an equivalent seed in
these APIs. Their graphs and floating-point kernels need not be identical.
Vecnook accumulates distances in `f64`; the external engines use their native
cosine implementations on the supplied `f32` vectors.

Engine order rotates: Vecnook/USearch/Chroma, then USearch/Chroma/Vecnook,
then Chroma/Vecnook/USearch. One benchmark child ran at a time. No build, plotting,
or other benchmark job from this driver overlapped the timed workloads. Host/editor
activity was otherwise uncontrolled; filesystem cache was not flushed. All nine
scheduled results are retained. Preflight checks of the oracle and Chroma's
configuration/schema preceded this scheduled comparison.

## API and implementation details

**Vecnook:** native `VectorIndex::search_hnsw`, invoked by `bench-file` in a release
binary built with thin LTO and one codegen unit. Its exact-search work runs outside
the HNSW timer. This path does not exercise the WAL, document decoding, generations,
SQ8, or the optional SIMD/mmap companion.

**USearch:** the Python binding's `Index.search` with `dtype="f32"`, `metric="cos"`,
`connectivity=16`, `expansion_add=200`, and `expansion_search=128`. Both insertion
and search explicitly use `threads=1`. See the [official Python API guide](https://github.com/unum-cloud/USearch/blob/main/python/README.md).

**Chroma:** a fresh local `PersistentClient` and collection per process, with
`embedding_function=None`. Requests supply embeddings directly. The configuration
sets `space="cosine"`, `max_neighbors=16`, `ef_construction=200`, `ef_search=128`,
and `num_threads=1`. `batch_size` and `sync_threshold` are both 5,183 so the full
corpus reaches the configured HNSW ingestion/sync threshold before measurement.
The driver verifies the applied configuration through the collection's serialized
embedding-index schema, which retains fields omitted by `collection.configuration`.
The API returns IDs and distances, with no documents, metadata, or filter payload.
Python list payloads are prepared before timing, while API validation and response
construction remain inside it. No HTTP server or cloud service is used. See
[Chroma's collection configuration](https://docs.trychroma.com/docs/collections/configure).

Chroma's ingestion includes database persistence while the index-library build
paths do not. Build times and whole-process RSS are therefore omitted from the
comparison chart; these runs do not establish a write-throughput or memory ranking.

## Independent reference and evidence

A separate NumPy `f64` cosine calculation sorts exact Top-10 by distance then ID.
Every ID and all 3,000 returned distances agree with the current Rust exact-search
output within 1e-12. USearch and Chroma are scored against this same independent
reference. Recall measures nearest-neighbor agreement, rather than relevance
against SciFact's human labels.

[Raw results, complete environment, per-child output, and hashes](https://github.com/gay00ung/vecnook/blob/main/docs/benchmarks/engine-comparison-2026-10-29.json)
contain the nine scheduled runs. The input hashes match the
[original data provenance](text-benchmarks.md#inputs-and-reproduction).

| Artifact | SHA-256 |
| --- | --- |
| Base fvecs | `4995c03f6910c437ae5a60c8c62392af28eeee33c486e978de6b3216b11783fb` |
| Query fvecs | `84735c492ff0e4c0e951e7f1d36cc728231192a497ccb2d93a2a0761edf6678e` |
| Independent exact JSON | `bf81022f6dff4f99bed52373b60fb17482e88052ec961c37420a8bc1c965e9fc` |
| Measured Vecnook binary | `7d4099c0fe112954c102ad57f7a3263b5ca5496a23ad80c535911688a1347de5` |

Compiler: `rustc 1.96.0 (ac68faa20 2026-05-25)`. Python: `3.12.11`. Full installed package
versions are recorded in the raw evidence. External packages were installed in
an isolated environment; they are not Vecnook runtime dependencies.

## Individual scheduled runs

| Round | Target | Recall@10 | p95 ms |
| ---: | --- | ---: | ---: |
| 1 | Vecnook | 99.9333% | 1.037792 |
| 1 | USearch | 99.8333% | 0.806584 |
| 1 | Chroma | 99.9000% | 2.842417 |
| 2 | USearch | 99.8333% | 0.924916 |
| 2 | Chroma | 99.9333% | 2.160083 |
| 2 | Vecnook | 99.9333% | 1.309875 |
| 3 | Chroma | 99.9000% | 3.095541 |
| 3 | Vecnook | 99.9333% | 2.062000 |
| 3 | USearch | 99.8333% | 1.197167 |

## Reproduce

From a macOS source checkout, with the frozen input files matching the hashes above:

```bash
python3.12 -m venv work/comparison-env
work/comparison-env/bin/pip install -r tools/comparison-requirements.txt
cargo build --locked --offline --release --bin vecnook --example ground_truth
mkdir -p work/comparison

target/release/examples/ground_truth work/scifact/base.fvecs work/scifact/query.fvecs > work/comparison/rust-truth.json
OMP_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 work/comparison-env/bin/python tools/compare_usearch.py \
  work/scifact/base.fvecs work/scifact/query.fvecs \
  --oracle work/comparison/truth.json --rust-truth work/comparison/rust-truth.json
work/comparison-env/bin/python tools/benchmark_comparison.py \
  work/scifact/base.fvecs work/scifact/query.fvecs \
  --truth work/comparison/truth.json --rust-truth work/comparison/rust-truth.json \
  --output work/comparison/results.json
```

Choose a fresh result path to preserve previous evidence. Input-generation steps
are in the original data provenance; model/backend differences can change vector
hashes and must be reported. To render the committed results, run
`python3 tools/render_comparison.py` with Matplotlib installed in the rendering environment.

This is one small text-embedding workload on one machine. It does not measure
server deployments, large corpora, concurrent throughput, persistence guarantees,
or production application behavior. Its run session differs from the
[Vecnook-only sweep](current-benchmarks.md); latency from those sessions should
not be mixed into this comparison.
