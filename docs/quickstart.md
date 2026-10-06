# First search in a development checkout

[Watch the 36-second recorded demo](media/offline-demo.mp4). It uses actual macOS CLI output, paced for reading.

The `codex/v1-readiness` branch contains the upcoming `0.4.0-beta.1` implementation. These commands require this checkout; the published `0.3.0-beta.2` CLI does not include `demo`.

```bash
cargo build --release --offline
./target/release/vecnook demo data/first-demo
./target/release/vecnook demo data/first-demo 1
```

On Windows use `target/release/vecnook.exe`. Select query 1, 2 or 3, or omit the number to run all three. The demo creates a named collection, stores the authored Markdown, checkpoints it and performs HNSW queries. A later invocation reopens it and prints the original source text. An existing directory containing unrelated files is refused.

The queries and document embeddings were generated ahead of time by the local EmbeddingGemma model. This is a prepared-query demonstration; it does not embed arbitrary text or require a network connection at runtime. The generator, model digest and authored document checksums are in `tools/generate_demo.py` and `docs/demo-provenance.json`. Model weights and user documents are not included.

The included vectors are generated outputs. [Gemma's terms](https://ai.google.dev/gemma/terms) distinguish outputs from model derivatives and state that Google claims no rights in generated outputs. The authored sample text and generator use this repository's MIT license.

To query your own documents with free text, continue with [the Markdown client](../examples/documents/README.md). That optional client uses a locally installed Ollama embedding model. The database itself stays dependency-free.
