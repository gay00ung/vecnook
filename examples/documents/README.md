# Search your Markdown documents

This demo turns Markdown into local embeddings, stores each chunk with its source path and line range, and returns original text for a natural-language query. Search opens the database in a new process, so the example also exercises persistence.

You need Python 3.10+, the Vecnook CLI, and [Ollama](https://ollama.com/download) running locally. The Python client uses only its standard library. Ollama supplies the embedding model; it is not a database dependency.

In 1.2.0, the installed CLI includes this client:

```bash
vecnook docs-sync ./my-markdown --db data/notes
vecnook docs-find "How do I recover my documents?" --db data/notes
vecnook docs-find "write_batch_if_sequence" --db data/notes --json
```

These commands work outside the checkout. `docs-find` defaults to automatic
vector/hybrid selection; `--mode keyword` skips embedding calls. The direct
Python `search` command below keeps its existing vector default and also accepts
`--mode auto|vector|keyword|hybrid`. See the [first-use guide](../../docs/quickstart.md#search-your-own-folder).

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

Pruning requires `--prune` and a complete successful scan. It removes missing
sources in the scanned formats: `.md` by default, plus `.txt` with
`--include-text`. Use `--dry-run` to review which sources would be removed.
Access errors, symlinks and model errors abort pruning. A file made empty still
removes its old chunks as an explicit update.

The stored model digest, prompts, dimensions and metric must match; a changed
model requires a new collection. Omit `sync --model` to infer the existing
model. A sequence precondition rejects intervening writes: rerun the sync after
a conflict. Successful batches are durable before completion is reported;
a checkpoint is separate maintenance. Completion reports changed, unchanged,
deleted and failed file counts.

## Complete application flow

With Ollama running and `embeddinggemma` installed, use a new application directory:

```bash
python3 examples/documents/workflow.py data/document-app --binary target/release/vecnook
```

This runnable command copies the five MIT samples into its own folder, imports them, checks unchanged sync, modifies a copy, searches by source and tag, reopens, makes an independent named backup, exports and restores to a new collection. Assertions compare original text and results after each restore. The input samples remain available for another run. Each CLI call closes its handle; a Rust app should retain one worker as shown in the [backend example](../../docs/app-integration.md).

The same sources and tools are in the Cargo archive. Extract the `.crate`, run the command from its root and pass `--binary` pointing to the CLI installed from that version. The optional model is separately installed; it is not bundled with the source archive.

When the model digest or its prompts change, create a new collection and run the initial import again. Query the new collection, verify originals and relevant results, then move the application's chosen collection path. Retain the old database and a checked backup until migration succeeds.

Run the offline integration tests with a locally built binary:

```bash
VECNOOK_BINARY=target/debug/vecnook python3 -m unittest discover -s examples/documents -p 'test_*.py'
```

## Optional static HTML input with your own embeddings

`html_jsonl.py` is a Python 3.10+ standard-library example that produces the existing
JSONL v1 collection header and one document. It reads local UTF-8 HTML and a JSON
vector supplied by your application. It makes no network requests and does not
select or download an embedding model.

First call `extract_text(html)` from this module and pass that exact string to
your application's embedder. Save its coordinates as a JSON array in `vector.json`.
Use the same model/version/digest and prompt identity for imports and queries:

```bash
python3 examples/documents/html_jsonl.py page.html --vector vector.json \
  --id 18446744073709551615 --model-identity 'my-model:revision:document-prompt-v1' \
  --tag html --output page.jsonl
vecnook docs-import-jsonl page.jsonl --db data/new-html
```

The output requires a new file, and the import requires a new collection. Existing
outputs and the two input files are preserved. `--source` defaults to the HTML
filename; pass a stable application source label when filenames are ambiguous.
`--metric` accepts `l2` (default), `ip` or `cosine`. IDs are canonical decimal u64
strings, including the full range. Coordinates retain the caller's JSON values;
the CLI rounds them to f32 on import, preserving maximum finite magnitudes and
negative zero. A cosine vector must remain nonzero after f32 rounding.

The extraction contract is deliberately small:

- Collect text nodes in parser order and decode character references with
  Python's [HTMLParser](https://docs.python.org/3/library/html.parser.html).
  Skip comments and the contents of `head`, `script`, `style`, `template` and
  `noscript`, including nested excluded elements.
- Insert one LF before and after each block tag listed in `html_jsonl.BLOCKS`
  when the current text does not already end with LF. This includes paragraphs,
  headings, lists, table cells, `br` and `hr`. Inline tags add no separator.
- Preserve spaces, tabs, CR and internal LF in text nodes, including indentation
  in `pre`. Remove only leading/trailing LF from the assembled text. Reject an
  empty or whitespace-only result. For example, `<p>A</p><p>B</p>` becomes `A\nB`;
  `<b>A</b>  B` becomes `A  B`. Source indentation between blocks remains text.
- The source range is the **whole HTML input**, from line 1 through the number of
  LF characters plus one, including the final empty line after a trailing LF.
  It is not an offset map between extracted text and HTML nodes.

This follows HTMLParser's permissive token stream, without browser DOM repair.
An unclosed excluded element suppresses subsequent text until its matching close.
CSS, `hidden` attributes, JavaScript, external resources, image alt text and link
targets are not interpreted. Review the extracted text before embedding when the
application needs rendered-page or accessibility semantics.

The HTML input is limited to 1 MiB and the vector file to 128 KiB, read as bytes
without universal-newline conversion. Extracted text is limited to 6,000 UTF-8
bytes; larger documents need application-owned chunking and distinct IDs. The
source label permits 1..1,024 UTF-8 bytes and model identity 1..512, without NUL.
Up to 32 distinct nonempty tags permit 128 UTF-8 bytes each, without NUL. Vectors
have 1..4,096 finite f32-roundable coordinates; JSON booleans, NaN and overflow
are rejected. The header and document row retain the existing 64/128 KiB JSONL
limits. Validation finishes before the output is opened; filesystem write errors
can still leave a partial new file to inspect. PDF parsing, automatic chunking,
folder synchronization and model calls are outside this example.

`test_html_jsonl.py` covers parser semantics, byte/metadata bounds, invalid inputs,
non-overwrite behavior and an installed CLI import/export/re-import. The CLI check
compares UTF-8 text, source lines, full-u64 ID, tags, model identity and f32 bits.
It is included in the offline test command above and runs from the Cargo archive.

## File dates, byte counts and Boolean filters

```bash
vecnook docs-sync ./my-markdown --db data/notes --record-metadata
vecnook docs-find "backup" --db data/notes --mode keyword \
  --where '(tag("rust") OR tag("notes")) AND number("file_bytes") > 100 AND date("modified") >= "2026-10-01"'
```

`--record-metadata` stores whole-file UTF-8 byte counts and UTC modification dates
on every chunk. A later sync infers this setting when omitted;
`--no-record-metadata` explicitly removes the attributes. Metadata-only changes
currently re-embed the affected source. Direct Python index/sync/search accepts
the same flags. Default new imports retain VDOC1. Attribute-bearing documents
use VDOC2 and require a 1.2 reader. See [Boolean and range semantics](../../docs/compound-filters.md).
