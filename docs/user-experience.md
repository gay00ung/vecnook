# Search and maintain your documents with 1.3

Vecnook 1.3 keeps the Rust core std-only and preserves default vector, keyword,
hybrid and storage contracts. The document client can preview a sync, reuse
unchanged embedding inputs, explain retrieval and return bounded original context.

## Install a native CLI

Download a ZIP and its matching `.zip.sha256` file from the
[candidate release](https://github.com/gay00ung/vecnook/releases/tag/v1.3.0-rc.1).
Choose the suffix that matches your machine:

| Host | Archive target suffix |
| --- | --- |
| Linux x86_64 with glibc | `x86_64-unknown-linux-gnu` |
| macOS Apple silicon | `aarch64-apple-darwin` |
| Windows x86_64 | `x86_64-pc-windows-msvc` |
| Windows ARM64 | `aarch64-pc-windows-msvc` |

On macOS, verify the download before extraction:

```bash
shasum -a256 -c vecnook-1.3.0-rc.1-aarch64-apple-darwin.zip.sha256
unzip vecnook-1.3.0-rc.1-aarch64-apple-darwin.zip
chmod +x vecnook
./vecnook demo data/first-demo
```

Linux uses `sha256sum -c` with its corresponding filename. On Windows, run
`Get-FileHash .\vecnook-1.3.0-rc.1-x86_64-pc-windows-msvc.zip -Algorithm SHA256`
in PowerShell and compare the full hash with the downloaded checksum file, then
extract the ZIP and run `./vecnook.exe demo data/first-demo`. Add the extracted
directory to PATH to call `vecnook` from elsewhere. For another Rust host, build
the source with `cargo install vecnook --version '=1.3.0-rc.1' --locked`.

## Prepare the local client

```bash
vecnook env-check --model embeddinggemma --json
vecnook docs-sync ./notes --db data/notes --dry-run --json
vecnook docs-sync ./notes --db data/notes
vecnook docs-find "How do I restore my backup?" --db data/notes --explain
```

`env-check` checks Python 3.10+, the selected local Ollama origin and the installed
model. With `--db`, it also checks the stored digest. Exit 1 with `ready=false`
means preparation is needed; its checks distinguish a missing server, missing
model and changed identity. It does not install software or download a model.
The prepared `demo` and low-level Rust commands need neither Python nor Ollama.
The embedded script is sent through stdin, avoiding Windows argv limits. Search
vector arguments additionally accept `@query.csv` (at most 128 KiB); document and
Python clients use temporary vector files for long coordinates. Batch files still
parse literal vectors and never resolve arbitrary `@` paths.

Document commands accept `--db collection-dir` in place of the root/name/model
prefix. For example, `docs-info --db data/notes`, `docs-export --db data/notes
data/notes.export` and `docs-backup --db data/notes backups notes-copy` infer the
immutable identity. Existing positional commands remain available. Each command
has `--help`. A global `--json-errors` before the command emits one JSON error on
stderr with `schema_version`, native `error.kind` and `error.message`. It does not
change successful command output. Errors from the Python client are surfaced as
`InvalidInput` with their original actionable message.

The native archive workflow produces tested CLI ZIPs and SHA-256 files for its
Linux x86_64, macOS ARM64, Windows x86_64 and Windows ARM64 runners. These binary
targets do not imply validation of every OS release, libc or CPU. The Cargo source
package remains the route for other supported Rust hosts. Archive receipts record
the actual build host and extracted demo/restart checks.

## Preview and reuse embeddings

`docs-sync --dry-run` scans the bounded input and inspects storage read-only. It
does not create a collection, trim an incomplete WAL frame, rebuild a persisted
cache, call a model or prune files. JSON reports each changed source, planned
deletions, reused/recomputed chunks, the namespace and current capacity. The
snapshot projection is a conservative upper bound including retained physical
records and planned puts. A new collection's dimensions and byte estimate remain
unknown until its first embedding. Use `env-check` separately to check the model.

Normal sync checks the model digest before reuse. It compares the exact model
input, including the EmbeddingGemma title prefix, against original stored chunks.
Unchanged chunks reuse original f32 vectors; date/tag-only edits call no embedding
endpoint. A renamed title or changed prompt/model may require recomputation even
when body text is unchanged. Current reuse is within each changed source; identical
new sources are not a global embedding cache.

Each source commits atomically using a sequence guard. Progress goes to stderr;
`--json` keeps stdout machine-readable. A failed run can have completed earlier
sources: rerun with the same namespace and inspect the reported source counts.
Pruning still requires `--prune` and stays within that namespace. I/O failures can
be ambiguous; inspect the affected original records before retrying. Limits remain
512 files, 1 MiB/file, 32 MiB total, 10,000 chunks and the existing native caps.

`docs-inspect --db data/notes` exposes the validated read-only document/vector
view, sequence, capacity and pending tail. It holds an exclusive read-only lock
while reading, so it refuses a collection already owned by an application. Use
the application's queued status method for an open collection. Inspection can
retain the bounded original dataset in memory and is not a streaming scan.

## Choose tokenization and original context

```bash
vecnook docs-find "백업" --db data/notes --mode keyword --analyzer korean-ngram
vecnook docs-find "restore backups" --db data/notes --group-by-source --context-chunks 1
vecnook docs-find "restore backups" --db data/notes --max-distance 0.5 --json
```

`words` remains the default tokenizer. `korean-ngram` adds two/three-syllable
Hangul grams while retaining whole words and ASCII identifiers. It can match
`백업` within `백업을`, at the cost of additional index memory/startup work and
potential unrelated partial matches. It is not a morphological analyzer, Unicode
normalizer, spell checker or general substring search. Select it on each CLI
keyword/hybrid request. Rust owners call `set_keyword_analyzer` once after open;
queries and subsequent mutations use the same rebuilt in-memory policy. Reopen,
import and backup start with the default; reapply the application preference.

Human output names the retrieval mode and text-selection reason. `--explain`
adds existing counts, vector execution reason and branch diagnostics. `auto`
still chooses vector retrieval for prose and hybrid retrieval for identifier
heuristics. Vector Auto is a separate exact/HNSW execution decision. Empty results
explicitly distinguish no eligible documents from no lexical/bounded candidates.

Grouping keeps the first candidate for each source. Grouping and distance bounds
request up to 100 candidates, then apply their final selection and K; they do not
guarantee exhaustive source grouping. A distance bound keeps only candidates with
a recorded vector distance at or below the chosen value, including in hybrid
mode; lexical-only candidates without a distance are omitted. Keyword-only mode
rejects this option. Tune bounds per metric and model; scores are not calibrated
confidence. `candidate_complete` retains the pre-selection count observation and
`complete` describes the final eligible-count target, never semantic relevance.

`--context-chunks 0..3` adds original adjacent chunks from the same source.
Adjacency orders by start line, end line and ID. Every context row must also pass
the source/tag/expression filter, including when an excluded row lies between two
eligible chunks. This preserves query eligibility; filters remain application
query constraints, not an authorization boundary.

## Chunking and model changes

The default `bytes-v1` chunker retains the existing exact byte/line behavior.
For a new collection, `docs-sync --chunker markdown-v1` separates headings and
long paragraphs, keeps small fenced blocks together and falls back to the normal
byte bound for oversized blocks/lines. Original UTF-8 bytes and line references
are preserved. `--include-text` additionally reads UTF-8 `.txt` files; it is
explicit on each scan. Both modes retain the existing source/chunk limits.

Structural chunking records identity input schema `prompt_version=2` and the
chunker name, so older convenience clients reject that new workflow instead of
silently re-chunking it. Native collection/storage readers still see the original
bounded documents and opaque identity. Default imports retain identity schema 1.

```bash
vecnook docs-reindex ./notes --db data/notes --to data/notes-new --model NEW_MODEL
```

Reindex creates a new collection from the selected folder, validates original
chunk text/source/line ranges and keeps the old collection untouched for rollback.
Switch the application's configured path only after inspecting the new results;
the command never rewrites that configuration. `--resume` explicitly continues a
partially created destination with the same settings. Pass tags/file-metadata
options for the new import: arbitrary manually attached attributes are not copied
from the old collection. Changed model/chunker settings are rejected in ordinary
sync. Keep the old model available while validating and rolling back.

## Exchange records and connect clients

```bash
vecnook docs-export-jsonl --db data/notes --output data/notes.jsonl
vecnook docs-import-jsonl data/notes.jsonl --db data/restored
```

The first JSONL line is a `vecnook_collection` schema-v1 header containing the
embedding space. Each later line holds a document with a canonical decimal-string
ID and original `vector`, text, source, lines, tags and optional numbers/dates.
Finite f32 values, including signed zero, round-trip through native parsing.
Imports enforce UTF-8, unique full-u64 IDs, dimensions, metric, document limits,
10,000 chunks, 512 MiB input and native snapshot/batch bounds. They validate and
stage all rows before native logical import claims a new destination. Existing
destinations/exports are refused. A storage failure can still leave an incomplete
artifact; this is not a cryptographically authenticated interchange format.

The [Python client](../clients/python/README.md) is a separately installable
standard-library subprocess package. It supports caller-provided embeddings with
identity validation, lexical/vector/text queries, inspection, sync and JSONL.
Its optional reranker returns a permutation of original candidate IDs; payloads,
filters and native scores are preserved. The callback is application code, not an
automatically selected network provider. Calls block and acquire a fresh handle.
The [Node.js example](../examples/javascript/search.mjs) uses asynchronous
`execFile` and preserves string IDs. Neither is an in-process binding.

## Keep one application owner

The public app queue now accepts `submit_keywords` and `submit_hybrid` with an
owned `QueryFilter`, plus `status`, `compact`, `maintain` and named `backup`.
`status` reads capacity/sequence/WAL recommendations through the owning handle.
`activity` observes queued/running operations without waiting behind maintenance.
The [local application example](../examples/local_app.rs) runs all three modes.

The queue remains bounded to 32 requests, and operations run sequentially.
Compaction/backup can delay later searches. Activity reports operation state,
not a fabricated percentage; there is no automatic background compaction.
`Busy` means no request was accepted. Closing a response channel does not cancel
accepted writes or maintenance. Poll from a GUI event loop and wait/join only on
background or shutdown code. Preserve ACK, sequence-conflict and ambiguous-I/O
handling described in [application integration](app-integration.md).
