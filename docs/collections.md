# Keep source documents and embedding models together

`Collection` stores original text, source identifiers, line ranges and tags alongside each vector. Named collections occupy separate directories under an application-owned root, hold independent locks, and can reuse document IDs.

Run the [checked Rust example](../examples/collections.rs) on a fresh root:

```bash
cargo run --offline --example collections -- data/projects
```

`Collection::create(root, name, config, model_identity)` creates a new collection. `Collection::open(root, name, &EmbeddingSpace)` requires exactly matching model identity, dimensions and metric. Include a model digest/version and task prompt convention in the identity: a mutable alias alone cannot detect changed weights. The Markdown demo binds the Ollama digest and prompt version automatically.

Names contain 1–64 ASCII letters, digits, hyphens or underscores. Traversal, separators and Windows reserved device names are rejected. Existing destinations and collection-directory symlinks are refused. Keep the root under the application's control; names do not sandbox concurrent filesystem manipulation. Name case follows the filesystem, and the stored name must match exactly.

## Queries and writes

An empty `DocumentFilter` uses unfiltered Auto search. Source equality and all-tag matching use posting lists, intersecting from the smallest list before vector search. Missing tags/sources return no candidates. Source and tags are exact UTF-8 strings; the engine does not interpret source identifiers as filesystem paths. `filter_evaluations` can be zero because eligibility comes from posting lists; it does not count intersections or all graph work.

`get` decodes original text and `vector` borrows original coordinates. `put`, `delete` and `write_batch` use synced WAL frames. Batch preflight validates every document/vector before changing storage. Same-ID operations observe earlier operations in that batch. Derived source/tag indexes change only after success and rebuild on open. After an I/O error, close/reopen and inspect affected IDs before retrying, as for `Database`.

`checkpoint`, `compact` and `backup` preserve collection identity. A backup can have a different name and includes its own checked header. Failed creation or backup can leave an incomplete directory; choose a fresh name to retry. Existing destinations are never overwritten.

## Payload bounds and persistence

The immutable, checksummed `collection.bin` binds name and embedding space. `describe` checks that header without locking or validating the vector files. `open` also recovers/locks the database, checks config compatibility and validates every active document payload. A missing/damaged header or malformed payload fails opening rather than being skipped.

Document text is bounded to 6000 UTF-8 bytes, a nonempty source to 1024 bytes, and tags to 32 unique nonempty strings of at most 128 bytes each. Lines start at one and end at or after the start. All fields combined must fit 16 KiB of encoded metadata, so simultaneous field maxima can exceed the combined bound.

`Document::to_payload` produces a versioned `VDOC1:` hex representation suitable for TSV even when text contains tabs/newlines. `from_payload` checks lengths, UTF-8, bounds, version and trailing bytes before allocating bounded fields. The encoded document ID must agree with the underlying record ID.

Low-level snapshot/WAL formats are unchanged. A raw `Database` can read collection vectors, but arbitrary metadata writes through it can invalidate the document schema. Use `Collection` for mutations. The older demo's JSON-only payloads are not typed collections; rebuild them into a fresh directory with the updated importer.
