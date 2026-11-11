# Vecnook Python client

Install the native CLI separately, then install this package with
`python -m pip install ./clients/python`. The client uses Python's standard library
at runtime and executes the local binary; it is not an in-process Rust binding.
`Client` calls block and each opens the collection, so do not use them while another
application owns that collection's exclusive handle. `SnapshotSession` retains a
separate read-only published generation until you close it.

Pair the Python **1.8.0rc1** wheel with the core **1.8.0-rc.1** CLI.
The stable wheel and CLI are available from the [1.7.0 release](https://github.com/gay00ung/vecnook/releases/tag/v1.7.0).

Versioned wheels are attached to the
[GitHub releases page](https://github.com/gay00ung/vecnook/releases).
After downloading the wheel, install it with
`python -m pip install ./vecnook-1.8.0rc1-py3-none-any.whl`.

```python
from vecnook import Client

db = Client.create("data/search", 3, "my-model-v1/prompt-v1")
db.put({"id": "18446744073709551615", "text": "백업을 복원합니다", "source": "notes.md"}, [1, 0, 0])
print(db.search_keywords("백업", analyzer="korean-ngram"))
print(db.search_vector([1, 0, 0]))
db.export_jsonl("data/search.jsonl")
restored = Client.import_jsonl("data/search.jsonl", "data/restored")
```

For an Ollama collection, `sync` and `search` connect to the locally installed
model. `put_text` instead accepts your own embedding callable and the exact
collection identity. Include model digest/version and prompts in that identity.
This package does not select a provider, download models or send telemetry.

`VecnookError.kind` carries native error categories. An I/O failure on a write
can mean persistence occurred before failure; inspect affected IDs before retry.
Optional rerankers return an ordering of original candidate IDs; original text,
filters, scores and source references remain authoritative.

## Reuse a published snapshot reader

Use this interface for an existing immutable generation store, published through
the [generation API](../../docs/engine-generations.md). The `path` is that store,
not a mutable `Client` collection directory. Embeddings remain caller-owned.

```python
from vecnook import SnapshotSession

with SnapshotSession("data/generations", binary="vecnook", timeout=30) as reader:
    print(reader.info())  # pinned generation/sequence and initial open timings
    reply = reader.search_vector([1, -0.0, 0], k=1,
                                 ids=["18446744073709551615"])
    print(reply["result"]["neighbors"])
    for reply in reader.search_batch(([1, 0, 0] for _ in range(100)), k=1):
        print(reply["result"]["generation"], reply["result"]["neighbors"])
```

One session uses one child process and pins one generation. A new publication
does not refresh it; open another session to see the new `CURRENT`. `info()`
returns a copy of the initial native ready event. Search returns the complete
native response envelope, with its `result` generation report. IDs and generation/
sequence values remain canonical decimal strings. Search neighbors contain
original metadata; use the native `snapshot-get` command when you need original
vectors. The optional `ids` list restricts eligible rows; it is not authorization.

Use `sq8=N` to build resident SQ8 codes once for that session. To reuse an
explicitly prepared cache, run `vecnook snapshot-prepare-sq8 STORE CACHE` first,
then pass `sq8=N, sq8_cache="CACHE"` and optionally `buffer_bytes=...`. Missing
caches fail instead of being built implicitly. The native reader validates the
cache and buffer's row-size requirement; its lease prevents pruning while live.
The `ready` event separates original-store open, code build and cache validation
timings. This API does not establish a speedup or a global RAM cap.

Calls block and serialize on each session; use a background task in a GUI.
`search_batch` consumes and yields one query at a time. Results yielded before a
later error remain valid; the batch is not atomic. Queries have at most 4,096
finite f32 coordinates, K in 1..10,000, at most 65,536 canonical u64 filter IDs
and a 1 MiB encoded request budget. `max_response_bytes` limits each response
line to 16 MiB by default (configurable up to 64 MiB), so a large result may need
a smaller K or a larger explicit response budget.

The `timeout` covers startup and each request's pipe I/O, including a blocked
write. A timeout or malformed response closes and reaps the child; the client
never retries on a newer generation. Native query errors preserve
`VecnookError.kind` and leave a healthy reader available for the next request.
Native idle expiry is 60 seconds by default; use `idle_ms` to choose 1..86,400,000.
Use `with` or `close()` for deterministic process/pin/cache-lease release.
Repeated `close()` calls are harmless; garbage collection is only a fallback.
