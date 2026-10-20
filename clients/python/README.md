# Vecnook Python client

Install the native CLI separately, then install this package with
`python -m pip install ./clients/python`. The client uses Python's standard library
at runtime and executes the local binary; it is not an in-process Rust binding.
Calls block and each opens the collection, so do not use them while another
application owns that collection's exclusive handle.

Versioned wheels are attached to the
[GitHub release](https://github.com/gay00ung/vecnook/releases/tag/v1.3.0-rc.2).
After downloading the wheel, install it with
`python -m pip install ./vecnook-1.3.0rc2-py3-none-any.whl`.

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
