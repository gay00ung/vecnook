# Choosing an embedded search engine

Vecnook targets Rust and local application developers who want one persistent
handle for original documents, embedding identity, source/tag filters, ordered
atomic updates, diagnosis, backup and independent restore. Its core is std-only,
uses its own HNSW graph and forbids unsafe code. Your application supplies
embeddings and decides when to checkpoint or compact.

Start with `Config::new(dimensions).with_metric(...)` and Auto search. Preserve
the model version and task prompts alongside the dimensions. Use exact results
to measure ANN recall for your own independent questions. Increase efSearch if
the measured target is missed; result count alone does not establish quality.
M and efConstruction affect graph construction cost and connectivity. The
[operating-range measurements](operating-range.md) describe tested settings,
data distributions, filters and memory scope.

Original text has a cost: it is retained in Rust objects and encoded into the
snapshot/WAL document payload. Coordinates alone do not describe process memory.
Updates retain old nodes until compaction, and compaction temporarily builds a
replacement index. Use `capacity()` for snapshot/node headroom and `doctor` for
read-only storage state; plan memory and maintenance with the [operations guide](operations.md).

| Application need | Choice to evaluate |
| --- | --- |
| Original documents, identity, filters and recovery inside a small Rust app | Vecnook and its two runnable application flows |
| SIMD, quantization, memory-mapped index views or index-only performance | [USearch's Rust API](https://github.com/unum-cloud/usearch/blob/main/rust/README.md) |
| Vector queries integrated with an existing SQLite schema and SQL | [sqlite-vec](https://alexgarcia.xyz/sqlite-vec/) |
| Network service, richer payload querying or hybrid search | [Qdrant](https://github.com/qdrant/qdrant) |

These are differences in supported interfaces and workloads, rather than a
universal ranking. The [real-text comparison](text-benchmarks.md) found USearch
faster on the measured SciFact workload. Vecnook currently keeps its index in
RAM and has one persistent owner; it has no quantization, mmap, replication,
embedding model, built-in BM25 or server protocol.

Try [the first search](quickstart.md), then the [Markdown app](../examples/documents/README.md#complete-application-flow)
or [Rust backend](app-integration.md). The repository demonstrates those flows;
it does not claim a completed GUI adapter or external app adoption.
