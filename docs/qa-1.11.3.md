# 1.11.3 candidate QA

With `docs-sync --chunker markdown-v1`, a short fenced Markdown block could
be divided at a heading inside the fence. The public 1.11.1 and 1.11.2 RC1
CLIs reproduced the split with different and shorter inner markers. The
fix keeps the opening marker and its length until a matching closing line.
The CLI regression checks both cases and a line with trailing content.
It checks the original UTF-8 text and line spans before and after the closer.

Core `1.11.3-rc.1`, optional companion `0.1.12-rc.1` and Python wheel
`1.11.3rc1` form the candidate. The default `bytes-v1` chunker, storage,
search, full-segment mmap, Rust 1.89 support and original f32 records remain
compatible. An explicit sync can update the chunks in affected files.

## Source candidate

- Package the exact clean candidate with Rust 1.89 and verify the fresh CLI
  and library consumers. Run 70 document tests on checkout and packaged CLIs.
  Check the fence regression, JSONL round trips and rejection rules, prune,
  SQLite and HTML, the Node app and isolated Python wheel.
- Run independent core, native cache/session and Python session workloads.
  Each run needs its own 86,400 seconds, at least 100,000 verified operations,
  successful exit, exact archive and binary identities, and lifecycle evidence.

## Registry and stable promotion

Follow [releasing](releasing.md) and [release readiness](release-readiness.md).
After normal PR and clean main CI, publish and independently download the core.
Use that exact core and checksum to package and verify the actual companion.
Pass the actual paired archive gate before publishing the companion.

Run four independent actual registry workloads with their own duration, count,
exit, archive and lifecycle checks. Bind the mapped reader to the final
successful core publisher receipt. Test both registry packages in fresh CLI
and library consumers with Rust 1.89 and stable on Linux x64, macOS ARM64,
Windows 11 ProductType 1 ARM64 NTFS and Windows Server ProductType 3 x64 NTFS.
Check the isolated wheel with the installed public CLI, exact docs.rs pages,
remote annotated tag installations, tag CI and eight native executables.
Verify every public asset digest and size, plus independent downloads and
SHA256SUMS. Stable promotion has separate PR and main CI, package equivalence,
core-first publication and final public asset verification.
