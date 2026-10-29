# Local engine comparison and distance scoring

Measured on **2026-10-29**, using the same frozen SciFact vectors for the original
Vecnook RC1-code binary, an improved **unpublished development build**, USearch
**2.26.6**, and Chroma **1.5.9**. The implementation is
[b66cf72](https://github.com/gay00ung/vecnook/commit/b66cf72b82b95bca9b670493d3ff857196edbe0f);
the measured source and driver revision is
[8ea578b](https://github.com/gay00ung/vecnook/commit/8ea578b8081b688affddbdd03f70d04302bc13b0).
Both local Vecnook binaries report 1.5.0-rc.1; the after binary contains subsequent
code and is identified by its commit and SHA-256. It is not the registered RC1 archive.

## Fresh before/after results

Each target ran in **eight fresh processes**, in two four-round rotations.
All **32 scheduled results** are retained. Values below are medians of process-level
metrics; query samples are not pooled. The whiskers are observed min–max ranges.

| Target | Measured call path | Recall@10 | Median p50 ms | Median p95 ms | p95 min–max ms |
| --- | --- | ---: | ---: | ---: | ---: |
| Vecnook before (RC1 code) | Rust index API | 99.9333% | 0.765270 | 1.006625 | 0.854042–3.280208 |
| Vecnook after (development) | Rust index API | 99.9333% | 0.417584 | 0.522938 | 0.414250–2.766875 |
| USearch 2.26.6 | Python index API | 99.8333% | 0.657396 | 0.849896 | 0.731500–1.265583 |
| Chroma 1.5.9 | Python local DB API | 99.9667% | 1.379709 | 1.759667 | 1.468667–19.132625 |

Vecnook's median p95 decreased **48.1%**, from
**1.007 to 0.523 ms**. Its median p50 decreased
45.4%. Recall, query completeness, and the number of
Vecnook distance evaluations stayed the same in every run.

![All retained before/after and external-engine measurements, with observed p95 and recall ranges](https://raw.githubusercontent.com/gay00ung/vecnook/main/docs/media/engine-comparison.png)

The improved development build had the lowest median p95 in this session. The APIs
have different scopes: Chroma includes local database validation, query handling,
and a Python response; Vecnook and USearch measure index calls. These are local
call-path measurements with substantial host variation. They establish no general
ranking of isolated index kernels or production systems.

## What the analysis found

A separate search-only diagnostic used macOS `sample` for five seconds after index
construction and warmup. Of 4,105 main-thread samples, **3,891 (94.8%)** had
`search_layer` at the top of the stack. The hottest source location was the
candidate-scoring call. Disassembly showed the original cosine reduction adding
coordinate products into one ordered `f64` accumulator. Multiplications already
used compiler-generated vector instructions; the ordered addition chain remained.
The [sample call graph](benchmarks/search-profile-before-2026-10-29.txt) and
[diagnostic identities and checks](benchmarks/search-optimization-evidence-2026-10-29.json)
are retained separately from the timed comparison.

Every actual USearch benchmark process reported **`hardware_acceleration=neon`**.
Its native distance counter averaged **1363.75** evaluations per query,
compared with **1,453.27** reported by Vecnook. The implementations construct and
traverse different graphs despite matching M and ef settings, and their native
counter definitions need not coincide exactly. USearch also uses its native
cosine kernel on `f32` data; Vecnook retains ordered `f64` accumulation.
These observations explain why equal HNSW settings do not mean equal work. The
profile identifies Vecnook's own bottleneck; it does not apportion the entire
cross-engine gap among arithmetic, graph layout, and traversal choices. See the
[versioned USearch API](https://github.com/unum-cloud/USearch/blob/v2.26.6/python/usearch/index.py)
and [metric implementations](https://github.com/unum-cloud/USearch/blob/v2.26.6/include/usearch/index_plugins.hpp).

## The change

The graph traversal now scores up to **four independent candidates together**.
A query coordinate is loaded and converted once for four distance accumulators,
so the processor can overlap independent additions. Each candidate keeps the
original coordinate order, `f64` precision, cosine clamping, and signed-zero
behavior. Candidate admission follows the original graph-link order. Query and
stored inverse norms were already cached before this change.

The implementation uses portable safe Rust, with no dependencies or architecture
intrinsics. The core retains `forbid(unsafe_code)` and Rust 1.89 support. Vectors
below 32 dimensions use the original single-candidate path to avoid setup cost.
The optimization targets graph distance scoring; exact scans and generation/SQ8
readers retain their existing paths. Storage formats, search settings, and original
vectors are unchanged. There is no new memory-reduction claim here.

A separately linked [trace harness](../tools/search_trace.rs) ran against both core
libraries: **11,664 synthetic searches and 600 SciFact exact/HNSW searches**.
The resulting 12,319-line transcripts are byte-identical, including ordered IDs,
`f64::to_bits` distances, metadata, completeness, reasons, distance counts, filter
counts, and graph statistics. Synthetic cases cover L2/cosine/inner product,
1–768 dimensions, batch tails, ties, full-u64 IDs, deletion/upsert, and filters.
Additional unit tests compare scalar and interleaved distance bits through 4,096
dimensions, including extreme finite values, subnormals, cancellation, and signed zero.
The exact SciFact JSON also remains byte-identical and agrees with the independent
NumPy reference within 1e-12.

## Conditions and variation

- Apple M4 Pro, 48 GiB RAM, arm64 macOS 26.6.2 (25G83).
- Rust `1.96.0 (ac68faa20 2026-05-25)`; release profile, thin LTO, one codegen unit, for both Vecnook binaries. Python 3.12.11; pinned external package versions are in the raw evidence.
- 5,183 original 768-dimensional `f32` vectors; 300 separate queries; cosine; Top-10; no quantization.
- M/connectivity/max_neighbors=16, efConstruction/expansion_add=200, efSearch/expansion_search=128, one HNSW worker.
- Ten warmup queries and 300 timed sequential calls per fresh process. Nearest-rank p50/p95 include result construction; recall/ID checks occur outside the timer.
- Embeddings, loading, graph construction, startup, network, concurrent clients, persistence throughput, and memory comparisons are excluded.

Each four-round rotation orders before/after/USearch/Chroma, then after/USearch/
Chroma/before, then USearch/Chroma/before/after, then Chroma/before/after/USearch.
A benchmark child runs alone within this driver. No driver build, profiling, or
plotting job overlaps the timed runs. Other host activity is uncontrolled, and
**three pre-existing 24-hour registry QA workloads remained running**. CPU affinity
was not set and filesystem caches were not flushed. A one-minute load-average
snapshot near the follow-up series was 73.93; a later snapshot was 17.57.

The first four-round series showed large variation, so a second fixed four-round
series followed with the same binaries and configuration. **Both series contribute
to the headline medians**. The 19.133 ms Chroma and 2.767 ms development-Vecnook
outliers remain in the ranges. One paired round had slower after p95. These
shared-host results are exploratory and do not establish a latency SLO.

| Series | Before median p95 ms | After median p95 ms | p95 reduction |
| ---: | ---: | ---: | ---: |
| 1 | 0.933521 | 0.779146 | 16.5% |
| 2 | 1.015166 | 0.522938 | 48.5% |

The candidate pool was fixed at 128, with no per-engine tuning or precision
relaxation. Vecnook uses seed 42; these USearch/Chroma APIs expose no equivalent
seed. Recall means agreement with exact nearest neighbors, rather than SciFact
human-label relevance. This small corpus on one machine does not cover larger
corpora, server concurrency, production applications, or the CLI/SQ8/mmap costs.

## Evidence and identity

- [All 32 results, environment, binary hashes, and aggregation](benchmarks/engine-comparison-before-after-2026-10-29.json)
- [First 16-run series](benchmarks/engine-comparison-series1-2026-10-29.json) · [Second 16-run series](benchmarks/engine-comparison-series2-2026-10-29.json)
- [Equivalence and profiling evidence](benchmarks/search-optimization-evidence-2026-10-29.json)
- [Original nine-run comparison](benchmarks/engine-comparison-2026-10-29.json) · [Historical RC1 candidate-pool sweep](current-benchmarks.md)

| Artifact | SHA-256 |
| --- | --- |
| Base fvecs | `4995c03f6910c437ae5a60c8c62392af28eeee33c486e978de6b3216b11783fb` |
| Query fvecs | `84735c492ff0e4c0e951e7f1d36cc728231192a497ccb2d93a2a0761edf6678e` |
| Independent exact JSON | `bf81022f6dff4f99bed52373b60fb17482e88052ec961c37420a8bc1c965e9fc` |
| Before binary, retained from original comparison | `7d4099c0fe112954c102ad57f7a3263b5ca5496a23ad80c535911688a1347de5` |
| After development binary | `d8fdd2cec9539b1eaad4988c678bfaf1379d16b4f1c6bc541b2a1c8719f8fc8b` |
| Either complete search trace | `503fd31455054f387fc790139ac6427386a230efc77c3aa67b7be5ccdb98fc13` |

Input hashes match the [original input provenance](text-benchmarks.md#inputs-and-reproduction).
The before binary is the exact retained original-comparison executable. Its runtime
sources match baseline commit `8fee479`. The after binary is built from the
implementation above. Both hashes were checked before and after each full series.
External packages are optional comparison tools and do not become core dependencies.

## Reproduce

Use the frozen input files matching the hashes above. From a clean checkout of the
improved source, with Rust 1.96.0 installed and the same compiler/profile for both builds:

```bash
python3.12 -m venv work/comparison-env
work/comparison-env/bin/pip install -r tools/comparison-requirements.txt
mkdir -p work/before-source

git archive 8fee4793d9abfea7c3391a3921142b1da99aff40 -o work/before.tar
tar -xf work/before.tar -C work/before-source
cargo +1.96.0 build --locked --offline --release --manifest-path work/before-source/Cargo.toml --target-dir work/before-target --lib --bin vecnook
cargo +1.96.0 build --locked --offline --release --target-dir work/after-target --lib --bin vecnook --example ground_truth
work/after-target/release/examples/ground_truth work/scifact/base.fvecs work/scifact/query.fvecs > work/rust-truth.json
OMP_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 work/comparison-env/bin/python tools/compare_usearch.py work/scifact/base.fvecs work/scifact/query.fvecs --oracle work/truth.json --rust-truth work/rust-truth.json

work/comparison-env/bin/python tools/benchmark_before_after.py work/scifact/base.fvecs work/scifact/query.fvecs --truth work/truth.json --rust-truth work/rust-truth.json --before-binary work/before-target/release/vecnook --after-binary work/after-target/release/vecnook --before-revision 8fee479 --rust-toolchain 1.96.0 --host-activity 'Describe your actual concurrent workloads' --output work/series1.json
# Repeat that fixed driver into work/series2.json, retaining the first series.
work/comparison-env/bin/python tools/merge_comparison.py work/series1.json work/series2.json --output work/combined.json
```

The recorded run additionally used `--before-evidence` to verify the retained original
binary against its original JSON. A rebuilt baseline has its own hash and is recorded
as rebuilt; it must still use the original runtime revision. The driver checks frozen
inputs, oracle agreement, versions, complete results, and baseline runtime source
identity. Choose fresh output paths. The hardware metadata step requires macOS.

For cross-version result tracing, compile the same harness against each built library:

```bash
rustc +1.96.0 --edition=2024 -O -C lto=thin -C codegen-units=1 tools/search_trace.rs --extern vecnook=work/before-target/release/libvecnook.rlib -L dependency=work/before-target/release/deps -o work/trace-before
rustc +1.96.0 --edition=2024 -O -C lto=thin -C codegen-units=1 tools/search_trace.rs --extern vecnook=work/after-target/release/libvecnook.rlib -L dependency=work/after-target/release/deps -o work/trace-after
work/trace-before work/scifact/base.fvecs work/scifact/query.fvecs > work/before-trace.txt
work/trace-after work/scifact/base.fvecs work/scifact/query.fvecs > work/after-trace.txt
cmp work/before-trace.txt work/after-trace.txt
```

Render the committed desktop/mobile charts with `python3 tools/render_comparison.py`
in an environment with Matplotlib installed. Timing and plotting run separately.

## Individual scheduled results

| Series | Round | Target | Recall@10 | p95 ms |
| ---: | ---: | --- | ---: | ---: |
| 1 | 1 | Vecnook before | 99.9333% | 0.854042 |
| 1 | 1 | Vecnook after | 99.9333% | 0.472667 |
| 1 | 1 | USearch | 99.8333% | 0.760125 |
| 1 | 1 | Chroma | 99.8667% | 1.722834 |
| 1 | 2 | Vecnook after | 99.9333% | 0.414250 |
| 1 | 2 | USearch | 99.8333% | 0.755708 |
| 1 | 2 | Chroma | 100.0000% | 1.468667 |
| 1 | 2 | Vecnook before | 99.9333% | 0.870042 |
| 1 | 3 | USearch | 99.8333% | 0.731500 |
| 1 | 3 | Chroma | 99.9333% | 1.486292 |
| 1 | 3 | Vecnook before | 99.9333% | 3.280208 |
| 1 | 3 | Vecnook after | 99.9333% | 2.766875 |
| 1 | 4 | Chroma | 100.0000% | 19.132625 |
| 1 | 4 | Vecnook before | 99.9333% | 0.997000 |
| 1 | 4 | Vecnook after | 99.9333% | 1.085625 |
| 1 | 4 | USearch | 99.8333% | 1.265583 |
| 2 | 1 | Vecnook before | 99.9333% | 1.001916 |
| 2 | 1 | Vecnook after | 99.9333% | 0.525292 |
| 2 | 1 | USearch | 99.8333% | 0.933167 |
| 2 | 1 | Chroma | 99.9000% | 6.864291 |
| 2 | 2 | Vecnook after | 99.9333% | 0.919875 |
| 2 | 2 | USearch | 99.8333% | 0.874125 |
| 2 | 2 | Chroma | 99.9333% | 14.709250 |
| 2 | 2 | Vecnook before | 99.9333% | 1.284333 |
| 2 | 3 | USearch | 99.8333% | 0.989583 |
| 2 | 3 | Chroma | 100.0000% | 1.796500 |
| 2 | 3 | Vecnook before | 99.9333% | 1.019000 |
| 2 | 3 | Vecnook after | 99.9333% | 0.512125 |
| 2 | 4 | Chroma | 100.0000% | 1.660125 |
| 2 | 4 | Vecnook before | 99.9333% | 1.011333 |
| 2 | 4 | Vecnook after | 99.9333% | 0.520583 |
| 2 | 4 | USearch | 99.8333% | 0.825667 |
