# Search your Markdown documents

This demo turns Markdown into local embeddings, stores each chunk with its source path and line range, and returns original text for a natural-language query. Search opens the database in a new process, so the example also exercises persistence.

You need Python 3.10+, the Vecnook CLI, and [Ollama](https://ollama.com/download) running locally. The Python client uses only its standard library. Ollama supplies the embedding model; it is not a database dependency.

From the repository root:

```bash
cargo build --release --offline
ollama pull embeddinggemma
python3 examples/documents/search.py --binary target/release/vecnook index examples/documents/sample --db data/documents
python3 examples/documents/search.py --binary target/release/vecnook search "How do I recover after a process crash?" --db data/documents
python3 examples/documents/search.py --binary target/release/vecnook search "How can I improve approximate search accuracy?" --db data/documents --json
```

Replace the sample directory with your own Markdown directory. Indexing requires a new database directory and refuses to overwrite one. Its final path component is the collection name: use 1–64 ASCII letters, digits, hyphens or underscores. Each query prints source-relative paths, line ranges, distances and the stored chunk text. Re-running the search command uses the persisted vectors; it does not re-embed documents.

The demo uses the public [document collection API](../../docs/collections.md). Add `index --tag rust --tag notes` to tag every imported chunk, or `search --tag rust --source recovery.md` to narrow results through indexed tags and source equality. The default model is `embeddinggemma:latest`. The demo stores its Ollama digest and refuses a query if that model has changed. EmbeddingGemma uses separate document/query prompts following the [model instructions](https://ai.google.dev/gemma/docs/embeddinggemma/inference-embeddinggemma-with-sentence-transformers). Other installed models can be selected with `index --model NAME`; the demo then passes raw text. Check that model's prompt conventions before relying on its search quality.

Requests go to `http://127.0.0.1:11434` by default. Only local HTTP origins are accepted, proxy settings are bypassed, and [Ollama's embedding API](https://docs.ollama.com/api/embed) uses `truncate: false` so oversized input fails visibly. Model files have their own license; Vecnook does not distribute them.

Limits: 512 UTF-8 Markdown files, 1 MiB per file, 32 MiB total, 10,000 chunks, and 3,000 UTF-8 bytes per chunk/query. Chunking follows lines, splitting long lines without breaking Unicode characters. It does not parse Markdown syntax or resolve links. Source text is a stored snapshot and may differ from a file edited after indexing. Importing many chunks uses multiple atomic batches, so interruption can leave a partial database; choose a new directory to retry.

Run the offline integration tests with a locally built binary:

```bash
VECNOOK_BINARY=target/debug/vecnook python3 -m unittest discover -s examples/documents -p 'test_*.py'
```
