# Development operating range

These measurements were performed on 2026-10-06 on an Apple M4 Pro, 48 GiB, arm64 macOS 26.6.2. Vecnook uses Rust 1.89 release builds, thin LTO, one codegen unit, M=16, efConstruction=200, seed=42, cosine distance and original f32 coordinates. They are workload observations, rather than latency guarantees or maximum capacity claims.

Each case uses 64 seeded centers, independent corpus/query jitter seeds 43/84 and 100 queries, K=10 (capped to eligible count). An independent scalar f64 oracle scores returned coordinates against the Kth distance, accepting ties within 1e-12. Exact search agreed on all results; duplicate IDs, excess results, filter leakage and deleted IDs are rejected. Empty eligibility has no recall denominator and is reported as `null`.

Every case ran in three fresh sequential processes with ten warmup queries per setting, without another local benchmark or QA workload. The baseline contains 39 runs; the raw 50k/128 filter calibration and the two changed document paths were then rerun three times each. Raw index/storage code stayed unchanged. Values below use those nine follow-up runs where applicable; corpus/query hashes matched the baseline. Profiling runs are excluded.

Document cases include an authored 552-byte original text per record, 100 source names and tags. Their full-data search applies the `all` tag (100% selectivity), so costs include original-text decoding and filter handling. Whole-process RSS includes input/query arrays, retained document fixtures, oracle buffers, index, allocator retention and temporary persistence/compaction buffers. Post-phase RSS is an instantaneous observation, not a sampled phase peak. The external OS peak covers the entire child process.

## Search and build

EF is chosen independently for the full corpus and each filter/churn condition until ≥99% Recall@10. EF max includes final settings for all nonempty conditions. Statistics are medians across three processes; minimum recall is the lowest final-condition value in any process.

| Mode | N | Dimensions | Initial / max EF | Minimum recall | p50 / p95 / p99 ms | Build with synced WAL, s | Peak RSS, MiB |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| docs | 1,000 | 768 | 64 / 64 | 100.00% | 0.146 / 0.171 / 0.186 | 0.703 | 41.3 |
| docs | 10,000 | 768 | 64 / 64 | 100.00% | 0.131 / 0.154 / 0.178 | 8.466 | 289.7 |
| raw | 1,000 | 128 | 64 / 64 | 100.00% | 0.026 / 0.032 / 0.042 | 0.164 | 7.1 |
| raw | 1,000 | 512 | 64 / 64 | 100.00% | 0.092 / 0.113 / 0.124 | 0.487 | 14.7 |
| raw | 1,000 | 768 | 64 / 64 | 100.00% | 0.142 / 0.173 / 0.186 | 0.713 | 21.6 |
| raw | 1,000 | 1536 | 64 / 64 | 100.00% | 0.271 / 0.312 / 0.327 | 1.309 | 54.3 |
| raw | 10,000 | 128 | 64 / 64 | 100.00% | 0.028 / 0.035 / 0.036 | 1.658 | 45.0 |
| raw | 10,000 | 512 | 64 / 64 | 100.00% | 0.088 / 0.103 / 0.109 | 6.046 | 121.8 |
| raw | 10,000 | 768 | 64 / 64 | 100.00% | 0.126 / 0.150 / 0.158 | 9.205 | 177.4 |
| raw | 10,000 | 1536 | 64 / 64 | 100.00% | 0.226 / 0.269 / 0.300 | 16.743 | 325.5 |
| raw | 50,000 | 128 | 512 / 1024 | 99.10% | 0.183 / 0.280 / 0.351 | 10.472 | 200.8 |
| raw | 50,000 | 512 | 128 / 128 | 99.90% | 0.328 / 0.458 / 0.487 | 39.313 | 474.2 |
| raw | 50,000 | 768 | 128 / 128 | 99.20% | 0.418 / 0.488 / 0.534 | 57.751 | 703.3 |

## Persistence costs

Churn deletes 10% of IDs and replaces another 10% using individual synced writes. Cached reopen follows a checkpoint; WAL recovery follows that churn; compaction rebuilds the remaining 90%. Disk counts final DB files, excluding exported benchmark inputs.

| Mode / N / dimensions | Checkpoint s | Cached reopen s | WAL recovery s | Compact s | Final DB disk MiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| docs / 1,000 / 768 | 0.034 | 0.013 | 0.087 | 0.580 | 3.8 |
| docs / 10,000 / 768 | 0.127 | 0.125 | 1.053 | 7.670 | 37.8 |
| raw / 1,000 / 128 | 0.025 | 0.002 | 0.017 | 0.128 | 0.5 |
| raw / 1,000 / 512 | 0.029 | 0.006 | 0.059 | 0.415 | 1.9 |
| raw / 1,000 / 768 | 0.031 | 0.008 | 0.087 | 0.620 | 2.7 |
| raw / 1,000 / 1536 | 0.036 | 0.015 | 0.162 | 1.150 | 5.4 |
| raw / 10,000 / 128 | 0.037 | 0.019 | 0.180 | 1.316 | 5.5 |
| raw / 10,000 / 512 | 0.076 | 0.056 | 0.754 | 5.103 | 18.6 |
| raw / 10,000 / 768 | 0.110 | 0.086 | 1.141 | 9.038 | 27.4 |
| raw / 10,000 / 1536 | 0.169 | 0.151 | 2.107 | 14.756 | 53.8 |
| raw / 50,000 / 128 | 0.104 | 0.090 | 1.913 | 8.432 | 27.3 |
| raw / 50,000 / 512 | 0.294 | 0.272 | 5.987 | 32.875 | 93.2 |
| raw / 50,000 / 768 | 0.412 | 0.375 | 8.906 | 50.007 | 137.2 |

| Case | After build RSS | After search RSS | After checkpoint RSS | After recovery RSS | After compact RSS |
| --- | ---: | ---: | ---: | ---: | ---: |
| raw / 50,000 / 768 | 348.0 MiB | 348.8 MiB | 514.3 MiB | 698.1 MiB | 698.3 MiB |
| raw / 10,000 / 1536 | 135.2 MiB | 135.5 MiB | 197.1 MiB | 263.7 MiB | 325.5 MiB |
| docs / 10,000 / 768 | 101.3 MiB | 101.5 MiB | 145.2 MiB | 208.6 MiB | 289.4 MiB |

## Filters and updates

Filters select 0%, 0.1%, 1%, 10% and 100% of the initial corpus. Selectivity after deletion is computed from actual eligible counts. The table shows the 50k/128 raw case and the 10k/768 document case after churn. Auto can scan small eligible sets exactly.

| Case | Filter | Eligible | EF | Recall | p95 / p99 ms | Exact queries / 100 |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| raw / 50,000 / 128 | none | 0 | 512 | n/a | 0.000 / 0.000 | 100 |
| raw / 50,000 / 128 | p001 | 50 | 512 | 100.00% | 0.005 / 0.005 | 100 |
| raw / 50,000 / 128 | p01 | 500 | 512 | 100.00% | 19.987 / 20.929 | 0 |
| raw / 50,000 / 128 | p10 | 5,000 | 1024 | 100.00% | 5.622 / 6.120 | 0 |
| raw / 50,000 / 128 | all | 45,000 | 512 | 99.10% | 0.331 / 0.409 | 0 |
| docs / 10,000 / 768 | none | 0 | 64 | n/a | 0.001 / 0.001 | 100 |
| docs / 10,000 / 768 | p001 | 10 | 64 | 100.00% | 0.026 / 0.028 | 100 |
| docs / 10,000 / 768 | p01 | 100 | 64 | 100.00% | 0.067 / 0.073 | 100 |
| docs / 10,000 / 768 | p10 | 1,000 | 64 | 100.00% | 1.124 / 1.265 | 0 |
| docs / 10,000 / 768 | all | 9,000 | 64 | 100.00% | 0.168 / 0.175 | 0 |

The initial 50k/128 run met 99.1% full-corpus recall at EF512, while its 10% filter reached 98.7%. The follow-up calibrates that filter separately; use the EF range above rather than assuming full-corpus settings establish filtered recall. No result leakage or underfill occurred in final conditions.

## Profiled improvement

- 1,000 documents, 100% tag filter: p95 median 0.183042 → 0.171041 ms (6.6% lower), same input hashes and 100% recall in three processes per version.
- 10,000 documents, 100% tag filter: p95 median 0.685042 → 0.153833 ms (77.5% lower), same input hashes and 100% recall in three processes per version.

A separate repeated-query profile identified ID validation/materialization and sorting in `search_ids` as the main cost for full-coverage document filters. Posting-list counts now establish when every active document satisfies every constraint, then use the global search path. Regression checks replace a document's source/tags, delete it and reopen to verify the full-coverage shortcut is reconsidered and excluded IDs stay absent. `capacity()` also uses existing counters instead of scanning physical records. Scalar f64 distance accumulation and original f32 storage are unchanged.

## Application clients

One public worker executes queries sequentially with EF128. Each client submits 32 requests and waits for its response before the next. QPS includes thread creation, queueing and responses; latency includes queue waiting. This is contention on one worker, not parallel search capacity.

| Documents | Clients | QPS | p50 / p95 / p99 ms |
| --- | ---: | ---: | ---: |
| 1,000 | 1 | 3570.0 | 0.273 / 0.301 / 0.378 |
| 1,000 | 4 | 3910.2 | 1.010 / 1.143 / 1.200 |
| 1,000 | 8 | 4001.9 | 1.971 / 2.192 / 2.354 |
| 10,000 | 1 | 6113.0 | 0.157 / 0.179 / 0.271 |
| 10,000 | 4 | 6957.6 | 0.571 / 0.587 / 0.592 |
| 10,000 | 8 | 7319.9 | 1.066 / 1.169 / 1.225 |

## Real authored questions

`tools/evaluate_demo.py` embeds five authored English Markdown documents and ten predeclared questions with local EmbeddingGemma. Five English and five Korean questions each achieved Hit@1=100% and MRR@3=1.0 using exact search. This is a small authored example, not an independent multilingual evaluation. It measures intended-document relevance separately from ANN recall. The model digest is `85462619ee721b466c5927d109d4cb765861907d5417b9109caebc4e614679f1`; the original f32 document/query hash is `194f8c671ea51fd05ee19b89ed95500220ca556de114c66423bb75ff6ac3a19a`. Questions and expected sources are public in the tool.

## USearch Rust comparison

The separate Rust harness uses the existing SciFact corpus: 5,183 genuine 768d document embeddings and 300 independent query embeddings, cosine/F32/K10, M16/efConstruction200, Rust 1.99 release, thin LTO and one codegen unit for both engines. USearch is pinned to 2.26.4 in an ignored workspace; its native runtime uses its own kernels and graph construction. Both processes retain the same input/query arrays and independent f64 oracle. Timed search includes wrapper result allocations; build is in-memory index construction without document storage/WAL. Twenty warmup queries per EF setting precede each process's 300 timed queries. Three fresh sequential processes per engine met ≥99% recall.

| Engine | EF | Min recall | Build s | p50 / p95 / p99 ms | Sequential QPS | Whole-process peak MiB |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| vecnook | 64 | 99.7667% | 6.086 | 0.370583 / 0.479250 / 0.513250 | 2678.5 | 39.52 |
| usearch | 64 | 99.6000% | 3.469 | 0.307500 / 0.385333 / 0.409000 | 3211.8 | 37.16 |

USearch was faster and used slightly less whole-process peak memory here. This Rust comparison replaces neither the historical Python measurements nor workload-specific application evaluation. Runtime/allocator allocations still differ. Vecnook's value is its document/persistence/application contract; these numbers do not establish a speed advantage.

## Reproduce and boundaries

```bash
cargo build --release --offline --example capacity_bench
python3 tools/run_capacity.py --output work/capacity
python3 tools/evaluate_demo.py --binary target/release/vecnook --output work/relevance
python3 tools/run_comparison.py --base data/scifact/base.fvecs --query data/scifact/query.fvecs --output work/usearch-rust
```

Use the [real-text preparation recipe](text-benchmarks.md) for SciFact and separately install the local model for authored relevance. `run_comparison.py` creates a separate pinned Rust workspace; core dependencies remain empty. Run commands sequentially and retain JSONL, input hashes, environment and process RSS outputs. At least three fresh runs are required.

The fixed Auto exact threshold is a starting point. The 500-eligible raw filter above performs much more graph work than its eligible count. Use `SearchOptions::default().with_exact_threshold(512)` to scan such a set exactly, or explicitly choose `SearchStrategy::Exact`. Calibrate the crossover using your own eligible counts and dimensions; higher EF can improve recall while increasing filtered traversal cost.

The physical-node cap is 100,000 and the snapshot cap is 256 MiB; both include old/deleted versions. 50k×1536 raw coordinates already exceed that byte cap, so the runner rejects that configuration before creating a database. Documents also consume encoded payload budget. Tested document workloads stop at 10k; the dimension/node maxima are input bounds, not characterized operating recommendations. Large-scale user embeddings, arbitrary filters, other hardware, network filesystems and hardware power loss require separate validation. Long-soak and Windows 11 stable-release evidence remain explicit [release gates](releasing.md).

Corpus/query SHA-256 pairs for each generated case:

| N / dimensions | Corpus | Query |
| --- | --- | --- |
| 1,000 / 128 | `b88bff4d0421c5a0235b55a91f658d25cb3753b66408fb0a70ba4e106aa160a9` | `b76ee2af8d0ede38e76e22067ae9dc16f187e78139d7b53c28f42bb358cba47a` |
| 1,000 / 512 | `c531da23a4d2ee0606488df96c6e5eeae9ddc8308f4fd010594d5436042e38b0` | `b0dc4b5421cdc91e08182ac6ee85c11562e675d10473dc39930ad8ccf83d0535` |
| 1,000 / 768 | `11b35884d21b6ada1a81c23db81ad59ca417f439206a01f0275ada0dc7082549` | `78055f6f6db720f94439195a82c0eb3ff49e98a52d6512b635fdd3f51fc43145` |
| 1,000 / 1536 | `0a6d9803701e394612152753e372a790f68ea8df2250e2436946d28609f8ca2c` | `a2e20198c0c0a527e43e2b79d5971817415d31b13145dc259395cc1ed5fdfbbf` |
| 10,000 / 128 | `684759249f3b620edf9aa86da1add21fc067ab1b2b290c21ca896e943c35121d` | `b76ee2af8d0ede38e76e22067ae9dc16f187e78139d7b53c28f42bb358cba47a` |
| 10,000 / 512 | `5537847800490f5925b200308d82fc7660d1c57816ff241b172b978a40db85a1` | `b0dc4b5421cdc91e08182ac6ee85c11562e675d10473dc39930ad8ccf83d0535` |
| 10,000 / 768 | `f45f48f452854812c446296de1e6ba1c097674d631fae6f726db97cb4f672187` | `78055f6f6db720f94439195a82c0eb3ff49e98a52d6512b635fdd3f51fc43145` |
| 10,000 / 1536 | `b4a60f188665f8ac87dae88d3abeb031ff649627637dc5d7e7658b37e35fd05a` | `a2e20198c0c0a527e43e2b79d5971817415d31b13145dc259395cc1ed5fdfbbf` |
| 50,000 / 128 | `9d6965dfbe7ee038d85d16d3042cd0c583c700429a4e78a508bd65ea577dc528` | `b76ee2af8d0ede38e76e22067ae9dc16f187e78139d7b53c28f42bb358cba47a` |
| 50,000 / 512 | `5bf07aa4d183981547512d7e08d4fb8db59dac15edbb08c39e8a5cb33dcaabb1` | `b0dc4b5421cdc91e08182ac6ee85c11562e675d10473dc39930ad8ccf83d0535` |
| 50,000 / 768 | `561931452c9005b4d1712ed35718725ac0ceeea56e66120cebe0ca119220f825` | `78055f6f6db720f94439195a82c0eb3ff49e98a52d6512b635fdd3f51fc43145` |

Measurement binary SHA-256: baseline `fbac1d81ff921063ac96e151b722d359b95888bf859357bc5cbd9459f2a38af1`, follow-up `9e4209b8c8854c52daa19a2a7a622d49c9644b318a7b3b34430524364fd923a9`, Rust comparison `86707e861947d942cb17e2ee7203f2a3fa657d6f57101740ce33699fbca59f5b`. SciFact inputs: corpus `4995c03f6910c437ae5a60c8c62392af28eeee33c486e978de6b3216b11783fb`, queries `84735c492ff0e4c0e951e7f1d36cc728231192a497ccb2d93a2a0761edf6678e`. Raw datasets and logs remain outside the package.
