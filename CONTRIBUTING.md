# Contributing

Vecnook is an early embedded database. Changes should preserve the observable contracts in the README, keep storage failures diagnosable, and provide evidence for search-quality or performance claims.

## Build and check

Use Rust 1.89 or later on macOS or Linux. The project uses only the Rust standard library; no dependency downloads are needed after cloning.

```bash
cargo fmt --check
cargo test --offline
cargo clippy --offline --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --offline --no-deps
cargo build --offline --release
cargo package --offline
```

For local uncommitted changes, package verification can use `--allow-dirty`. CI also checks the declared minimum Rust version. Keep unsafe code and external search libraries out of the implementation.

## Report a problem

Open an issue with the version or commit, OS/architecture, Rust version, metric, dimensions, M, efConstruction, efSearch, and a minimal runnable example. Explain expected and actual behavior, including error messages and `SearchReport` or `RecoveryInfo` fields when relevant. For persistence issues, distinguish acknowledged writes from writes that returned errors.

Use generated or shareable example data. A quality report should compare HNSW against exact results for independent queries and state filter selectivity. `complete=true` reports the result count, not search recall. Performance reports need release builds, hardware, corpus/query provenance, K, warmup policy, and timed scope. Include build/recovery costs when those are the problem.

## Propose a change

Describe the concrete behavior before and after the change. Add regression coverage for correctness or recovery changes. Persistence changes need compatibility tests and a clear format/version transition; derived graph caches must remain disposable. Explain allocation and peak-memory costs for new data structures.

Run the checks above and update public documentation when the API or CLI changes. Do not commit local datasets, runtime databases, benchmark scratch files, or private planning/QA notes. The package manifest has an explicit file allow-list.

Submit contributions under the repository's MIT license. Benchmarks in this repository compare Vecnook with its own exact reference unless explicitly documented otherwise; do not present those results as comparisons with another engine.
