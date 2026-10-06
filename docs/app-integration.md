# A persistent backend for local Rust applications

Open one `Collection` when the application starts and reuse it. Each open holds an exclusive filesystem lock, so opening the same directory for each UI request fails while the application owns it. Put slow search and persistence work on a background thread.

The runnable [`local_app.rs`](../examples/local_app.rs) example uses the public `vecnook::app` module. It creates or reopens a named collection, moves it onto one worker, runs four application clients, checks the returned document, drains pending work and checkpoints before closing:

```bash
cargo run --offline --example local_app -- data/local-app
cargo run --offline --example local_app -- data/local-app
```

Both runs enqueue a live atomic document update, return its new text from `recovery.md`, delete a temporary note, checkpoint, drain and reopen. These three-dimensional vectors are a fixture for the integration contract. For text-derived embeddings, use the [complete local Markdown workflow](../examples/documents/README.md#complete-application-flow).

## Requests and ownership

`start(collection)` returns a cloneable `SearchClient` and a worker `JoinHandle<Collection>`. The worker owns the only persistent handle. Searches, writes and checkpoints execute sequentially in one queue; multiple callers can submit at the same time. Sequential submissions from one caller preserve that caller's order. Racing callers must coordinate ordering themselves. A search submitted after an accepted write executes after that write finishes. Inspect both responses: acceptance means queued, and a successful write response means acknowledged persistence.

Use `submit_batch(model, Vec<WriteOperation>, expected_sequence)` for owned `Put { document, vector }` and `Delete { id }` requests. Each batch is atomic. `Some(sequence)` rejects stale writes with `Error::Conflict`; `None` writes at the current state. The app queue accepts 1–64 operations and at most 256 KiB of encoded documents/coordinates per batch. Larger imports use the collection API outside the event loop. Model, dimensions and payload bounds are checked before enqueueing; vector values, capacity and sequence conflicts are validated by the worker and reported through the receiver. `checkpoint()` uses the same queue and returns a result receiver.

`SearchClient::submit` takes the expected model identity, an owned vector, K, an optional exact source filter and required tags. It uses `try_send` on a 32-request queue and returns a one-result receiver. In an event loop, poll the receiver with `try_recv`, or wait on a background task and dispatch the result back to the UI. Calling blocking `recv` or `join` on the UI thread would freeze it. Obtain and validate query embeddings off the UI thread as well.

Queries cap K at 100, tags at 32, source at 1,024 UTF-8 bytes and each tag at 128 bytes. Dimensions and the full model identity must match the collection. At 4,096 dimensions, 32 queued queries hold up to 512 KiB of coordinates plus strings. Maximum-size writes hold up to 8 MiB of encoded-equivalent payload across the queue, plus Rust objects, temporary encodings, channels and receivers. The active request, callers and results retain additional memory. Closing a receiver does not cancel an accepted search, write or checkpoint.

| Outcome | Caller behavior |
| --- | --- |
| `SubmitError::InvalidRequest` | Fix the model identity, dimensions or request limits before retrying |
| `SubmitError::Busy` | Keep the UI responsive and retry later or discard an outdated query |
| `SubmitError::Stopped` | Stop submitting; inspect worker termination |
| `Error::InvalidInput` through the receiver | Correct a non-finite or zero cosine vector |
| `Error::Io` / `Error::Poisoned` from a write | Stop submissions, drain/close, reopen and inspect affected IDs before retrying ambiguous writes |
| Response channel disconnects | Inspect worker termination; no result was delivered |

Drop every client clone to close submissions. The worker drains queued requests and exits; join it from application shutdown/background code to recover the collection. Checkpoint if needed, then drop the collection to release its lock. The example's tests exercise eight clients, the held lock, rejected requests, queue saturation, abandoned receivers, queued work during shutdown and reopening original documents.

## Connecting a desktop frontend

The backend uses the Rust standard library and has no GUI framework dependency. A Tauri, egui or other desktop application can own the `SearchClient` in its backend state and translate responses into its own frontend messages. Import `vecnook::app`; no helper copying is required. See the [compatibility contract](compatibility.md).

For a JavaScript frontend, encode document IDs as decimal strings: Vecnook supports `u64::MAX`, which exceeds JavaScript's exact integer range. Return original text, source, line range and distance separately. Treat source and document text as data and escape them when rendering. Store collections under an application-owned data directory; do not pass arbitrary frontend paths to storage operations. This repository tests the Rust backend; it does not include or claim validation of a GUI window or framework adapter.

## Platform scope

Persistent storage is supported on Linux, macOS and Windows with Rust 1.89 or later. On Windows, directory flush uses `OpenOptionsExt::custom_flags(FILE_FLAG_BACKUP_SEMANTICS)` and write access; a read-only handle cannot supply the required flush contract. Failures are returned rather than silently disabling synchronization. See Microsoft's [directory handle rules](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew) and [FlushFileBuffers access requirements](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers).

CI exercises persistent CRUD, locking, recovery, checkpoint, compaction, backups, document search and this backend on all three systems. Windows CI uses NTFS on Windows Server 2025. Network shares, other filesystem implementations and power-cut durability are outside the validated scope. Keep verified backups and investigate a reported synchronization error before writing again.
