# 1.11.2 candidate QA

The public 1.11.1 CLI wrote a 601,343,079-byte JSONL archive from an
isolated synthetic collection with 9,000 documents and 4,096 coordinates.
The importer refused the file at 512 MiB before claiming a destination.
Native export, import and re-export matched byte for byte for all records.

The fix measures exact UTF-8 bytes, including the header and line endings,
before creating an export. The count limit stays at 10,000 documents.
The candidate pairs core `1.11.2-rc.1`, Python wheel `1.11.2rc1` and
companion `0.1.11-rc.1`. It preserves Rust 1.89, dependency-free core,
original text, f32, JSONL v1, storage, default search and full-segment mmap.

## Source candidate

- Freeze the exact clean feature commit and package with Rust 1.89. Check
  archive contents, source fingerprints and fresh CLI/library consumers.
- Check the shared byte boundary with multibyte UTF-8. Use the actual CLI
  with the 9,000-document fixture and require a nonzero export code, absent
  output and an intact source. Restore a small export and compare all rows.
  Check the checkout, packaged and actual registry binaries separately.
- Run 69 document tests, including 10,000/10,001, max-f32, prune, eight SQLite
  and nine HTML tests. Bound the large restore timeout on CI and verify
  the isolated Python wheel and Node app.
- Run fresh mixed core, native cache/session and Python session QA. Each
  requires its own 86,400 seconds, 100,000 verified operations, exit 0,
  exact artifact hashes and lifecycle evidence.

## Registry candidate and stable promotion

Follow [releasing](releasing.md) and [release readiness](release-readiness.md).
Verify normal PR CI and merge, then exact clean-main CI. Publish and
independently download the core. Package and verify the actual companion
against the public core checksum and pass the actual pair gate before
companion publication. Review adapters serve package review only.

Run four separate actual-registry workloads with the same own-duration,
count, exit, hash and lifecycle requirements. The mapped reader binds the
final successful core publisher receipt. Verify both fresh registry CLI
and library consumers on Linux x64, macOS ARM64, Windows 11 ProductType 1
ARM64 NTFS and Windows Server ProductType 3 x64 NTFS with Rust 1.89 and stable.

Verify the isolated wheel with the public CLI, exact docs.rs pages, fresh
annotated remote tag installations, tag CI and eight real native binaries.
Check server digests and sizes, independent unauthenticated downloads and
SHA256SUMS. Stable promotion requires its own PR/main CI and official
equivalence, core-first paired publication and final public audit.
