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

Limits: 512 UTF-8 Markdown files, 1 MiB per file, 32 MiB total, 10,000 chunks, and 3,000 UTF-8 bytes per chunk/query. Chunking follows lines, splitting long lines without breaking Unicode characters. It does not parse Markdown syntax or resolve links. Source text is a stored snapshot. Each source is committed in one atomic batch, limited to 1,024 operations, 8 MiB encoded WAL payload and 16 MiB TSV input. A file exceeding a limit fails without replacing its old chunks. Several files commit separately: interruption can leave some files updated and others unchanged. Rerun `sync` to reconcile; completed sources are derived from stored documents, without an external completion manifest.

## Incremental updates

```bash
python3 examples/documents/search.py --binary target/release/vecnook sync examples/documents/sample --db data/documents
python3 examples/documents/search.py --binary target/release/vecnook sync examples/documents/sample --db data/documents --prune
```

Use the same user tags as the original import. Unchanged source/chunk content, line ranges and tags skip embedding calls. Changed sources replace old chunks atomically; matching chunk positions keep their IDs and new positions get unused IDs after the existing source IDs. Removed positions disappear. IDs are allocated against full stored IDs, with collision checks rather than a hash. A rename is a new source plus a missing old source; `--prune` removes the old one.

Imports store a reserved `vecnook:sync:v1:` tag identifying the managed folder. User-facing search hides this management tag; the collection API retains it. The default namespace derives from the absolute folder path. For portable folders, supply the same `--namespace my-notes` on the initial `index` and every `sync`. This namespace must own one source folder in the collection. Collections made by the older importer have no management tag: build a new collection rather than silently adopting their records. Unmanaged source collisions are rejected, and pruning only removes sources in the selected namespace.

Pruning requires an explicit flag and a complete successful directory scan. Access errors, symlink directories/files and model errors abort it. Changing a file to empty removes its old chunks as an explicit update. Deleting a file requires `--prune`. The stored model digest, prompts, dimensions and metric must match; a changed model requires a new collection. Omit `sync --model` to infer the existing model. A sequence precondition rejects intervening writes: rerun the sync after a conflict. Successful batches are durable before the command reports completion; a checkpoint is separate maintenance. Completion reports changed, unchanged, deleted and failed file counts.

Run the offline integration tests with a locally built binary:

```bash
VECNOOK_BINARY=target/debug/vecnook python3 -m unittest discover -s examples/documents -p 'test_*.py'
```
