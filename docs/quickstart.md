# First search after installation

[Watch the 36-second recorded demo](media/offline-demo.mp4). It uses actual macOS CLI output, paced for reading.

Install the exact release version with Rust 1.89 or later, then run the prepared
queries. The demo needs no embedding server or network connection after installation.

```bash
cargo install vecnook --version '=1.0.0' --locked
vecnook demo data/first-demo
vecnook demo data/first-demo 1
```

Select query 1, 2 or 3, or omit the number to run all three. The demo creates a named collection, stores the authored Markdown, checkpoints it and performs HNSW queries. A later invocation reopens it and prints the original source text. An existing directory containing unrelated files is refused. In a source checkout, `cargo build --release --offline` builds the same executable under `target/release` (`vecnook.exe` on Windows).

The queries and document embeddings were generated ahead of time by the local EmbeddingGemma model. This is a prepared-query demonstration; it does not embed arbitrary text or require a network connection at runtime. The generator, model digest and authored document checksums are in `tools/generate_demo.py` and `docs/demo-provenance.json`. Model weights and user documents are not included.

The included vectors are generated outputs. [Gemma's terms](https://ai.google.dev/gemma/terms) distinguish outputs from model derivatives and state that Google claims no rights in generated outputs. The authored sample text and generator use this repository's MIT license.

To query your own documents with free text, continue with [the Markdown client](../examples/documents/README.md). That optional client uses a locally installed Ollama embedding model. The database itself stays dependency-free.
