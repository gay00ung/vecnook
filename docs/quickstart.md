# First search after installation

[Watch the 36-second recorded demo](media/offline-demo.mp4). It uses actual macOS CLI output, paced for reading.

Install the exact release version with Rust 1.89 or later, then run the prepared
queries. The demo needs no embedding server or network connection after installation.

```bash
cargo install vecnook --version '=1.2.0-rc.1' --locked
vecnook demo data/first-demo
vecnook demo data/first-demo 1
```

Select query 1, 2 or 3, or omit the number to run all three. The demo creates a named collection, stores the authored Markdown, checkpoints it and performs HNSW queries. A later invocation reopens it and prints the original source text. An existing directory containing unrelated files is refused. In a source checkout, `cargo build --release --offline` builds the same executable under `target/release` (`vecnook.exe` on Windows).

The queries and document embeddings were generated ahead of time by the local EmbeddingGemma model. This is a prepared-query demonstration; it does not embed arbitrary text or require a network connection at runtime. The generator, model digest and authored document checksums are in `tools/generate_demo.py` and `docs/demo-provenance.json`. Model weights and user documents are not included.

The included vectors are generated outputs. [Gemma's terms](https://ai.google.dev/gemma/terms) distinguish outputs from model derivatives and state that Google claims no rights in generated outputs. The authored sample text and generator use this repository's MIT license.

To query your own documents with free text, continue with [the Markdown client](../examples/documents/README.md). That optional client uses a locally installed Ollama embedding model. The database itself stays dependency-free.

## Search your own folder

Install the Vecnook 1.2 candidate with Rust 1.89+. Python 3.10+ and
[Ollama](https://ollama.com/download) supply the Markdown/embedding client. Start
Ollama locally (`ollama serve` if its desktop app is not already running), then:

```bash
cargo install vecnook --version '=1.2.0-rc.1' --locked
ollama pull embeddinggemma
vecnook docs-sync ./my-markdown --db data/notes
vecnook docs-find "How do I recover my documents?" --db data/notes
vecnook docs-find "write_batch_if_sequence" --db data/notes --json
```

Replace `./my-markdown` with your Markdown folder and choose a new collection path.
No checkout or script path is needed after installation. The first sync creates
storage and records model identity, dimensions and prompts. Later syncs infer
those settings and skip unchanged files. Use `--model NAME` to choose another
installed model, `--tag project` to tag chunks, and a stable `--namespace notes`
when moving a folder. `--prune` explicitly removes missing sources from that
namespace. Source files are read without modification.

`docs-find` defaults to automatic text-mode selection: identifier questions use
hybrid ranking; prose questions use vectors. Override with `--mode hybrid`,
`--mode vector` or `--mode keyword`. Keyword queries work without a running model
after importing. `--source relative/path.md` and repeated `--tag` flags narrow
both branches. JSON includes original text, source, lines, decimal string IDs and
ranking diagnostics. See [ranking contracts](hybrid-search.md).

The first-search target is five minutes for a small folder. Model download and
embedding time depend on the machine and folder; use the [measured results](validation-1.1.md)
rather than treating that target as a latency guarantee.

## Filter by file attributes

```bash
vecnook docs-sync ./my-markdown --db data/notes --record-metadata
vecnook docs-find "backup" --db data/notes --mode keyword \
  --where 'number("file_bytes") >= 100 AND date("modified") >= "2026-10-01"'
```

Metadata is opt-in and uses VDOC2, which requires Vecnook 1.2 document readers.
Later syncs infer the setting; `--no-record-metadata` removes the attributes.
See [Boolean and range filters](compound-filters.md) for missing-field, date and
reader compatibility semantics. Existing default imports keep VDOC1.
