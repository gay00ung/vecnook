# Real text embeddings and a reference engine

This evaluation uses 5,183 [BEIR SciFact](https://github.com/beir-cellar/beir/wiki/Datasets-available) abstracts and 300 independent claims from the official test split. Both engines receive identical 768-dimensional `f32` vectors generated locally with EmbeddingGemma. It measures approximate nearest-neighbor agreement with exact cosine Top-10, not factual verification or relevance against SciFact's human labels.

## Inputs and reproduction

From a source checkout with Rust 1.89+, Python 3.10+ and local Ollama:

```bash
ollama pull embeddinggemma
python3 tools/prepare_embeddings.py --output work/scifact
cargo build --offline --release --examples --bin vecnook
target/release/examples/ground_truth work/scifact/base.fvecs work/scifact/query.fvecs > work/scifact/rust-truth.json
python3 tools/measure_vecnook.py work/scifact/base.fvecs work/scifact/query.fvecs --ef 64
```

The generator verifies BEIR's published ZIP MD5 and records SHA-256, corpus/query IDs, model digest, prompts and actual output counts. It reads named ZIP members without extracting paths. The preprocessing keeps at most 400 UTF-8 bytes of title and 4,000 bytes of abstract, preserving character boundaries. Seventeen abstracts were shortened. The model API rejects token overflow (`truncate=false`). Embedding generation and downloads are excluded from search/build measurements.

The model digest was `85462619ee721b466c5927d109d4cb765861907d5417b9109caebc4e614679f1`. Prompts were `title: TITLE | text: ABSTRACT` and `task: search result | query: CLAIM`. Input hashes:

| Artifact | SHA-256 |
| --- | --- |
| BEIR ZIP | `536e14446a0ba56ed1398ab1055f39fe852686ecad24a6306c80c490fa8e0165` |
| Base fvecs | `4995c03f6910c437ae5a60c8c62392af28eeee33c486e978de6b3216b11783fb` |
| Query fvecs | `84735c492ff0e4c0e951e7f1d36cc728231192a497ccb2d93a2a0761edf6678e` |

Different model backends/versions can produce different floating-point outputs. Keep the generated manifest with your own run. Dataset text and model weights are not redistributed here; [SciFact's authors](https://github.com/allenai/scifact) and the model retain their own terms and attribution.

## Independent oracle and optional comparison

The comparison environment is separate from the database. Its packages are never Rust dependencies:

```bash
python3 -m venv work/reference-env
work/reference-env/bin/pip install -r tools/reference-requirements.txt
work/reference-env/bin/python tools/compare_usearch.py work/scifact/base.fvecs work/scifact/query.fvecs --oracle work/scifact/truth.json --rust-truth work/scifact/rust-truth.json
work/reference-env/bin/python tools/compare_usearch.py work/scifact/base.fvecs work/scifact/query.fvecs --truth work/scifact/truth.json --ef 64
```

A NumPy `f64` cosine calculation independently agreed with every Rust Top-10 ID and all 3,000 returned distances within 1e-12. Ties sort by ID. USearch results are compared to that same independent ground truth.

Both configurations use connectivity/M=16, insertion expansion/efConstruction=200, `f32` coordinates, cosine, K=10, ten warmup queries and sequential single-query calls. Vecnook accumulates distance in `f64` with seed 42. USearch 2.26.4 uses its `f32` cosine kernel through the Python binding, NumPy 2.5.3 and NumKong 7.8.5. Its binding exposes no seed setting; insertion/search use `threads=1`. Search timing includes result construction through each language's API. Files, embedding calls, exact-oracle work and graph construction are outside timed queries; build time includes graph insertion.

## Measured results

Apple M4 Pro, 48 GiB RAM, macOS 26.6.2, Rust 1.99 release build (thin LTO, one codegen unit), Python 3.12.11. Filesystem cache was warm; this was not a cold-disk or concurrent load test. A sweep chose efSearch=64 as the smallest tested pool meeting ≥99% Recall@10 for both engines. The table reports medians of three fresh process runs at that setting:

| Engine/API | Recall@10 | p50 ms | p95 ms | Sequential QPS | Build s | Peak process RSS MiB |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Vecnook Rust | 99.7667% | 0.410958 | 0.534209 | 2394.08 | 6.478153 | 39.08 |
| USearch Python | 99.6000% | 0.347875 | 0.433000 | 2840.83 | 3.888249 | 90.92 |

USearch was faster in this comparison. The RSS columns have different runtime overhead: Vecnook includes its native benchmark/loader/queries/index/oracle IDs; USearch includes Python, NumPy, input arrays, the binding and native index. These numbers do not isolate index memory and cannot establish a core-memory advantage. The RSS scripts use POSIX resource accounting and are intended for Linux/macOS.

Single-run sweep results, useful for choosing the quality target:

| efSearch | Vecnook recall | Vecnook p95 ms | USearch recall | USearch p95 ms |
| ---: | ---: | ---: | ---: | ---: |
| 16 | 96.5000% | 0.227125 | 95.2333% | 0.181000 |
| 32 | 98.8667% | 0.334417 | 98.3000% | 0.269500 |
| 64 | 99.7667% | 0.490916 | 99.6000% | 0.478000 |
| 128 | 99.9333% | 0.800584 | 99.8333% | 0.766250 |
| 256 | 100.0000% | 1.294417 | 100.0000% | 1.101208 |
| 512 | 100.0000% | 1.948958 | 100.0000% | 1.748042 |

All measured queries returned ten candidates. This small scientific-text corpus is one distribution; larger corpora, other embeddings, selective filters and concurrency can change the result. Peak configured scale and hardware power-loss behavior remain uncharacterized.

## Operational regression checks

`tests/crash_recovery.rs` starts real child processes, waits for acknowledged setup, then kills them after observing WAL growth or temporary snapshot creation during checkpoint/compaction. Every acknowledged vector/payload must recover. The unacknowledged atomic batch must recover entirely or not at all. Observation-triggered kills exercise transitions around file writes; they do not certify a precise hardware flush boundary.

The suite also retains every-byte torn-frame tests, complete corruption rejection, 900 model-checked mutation/maintenance/restart operations, and typed collection batch/reopen/backup checks. Process interruption and checksum injection are evidence for those contracts, not a substitute for hardware power-loss testing.
