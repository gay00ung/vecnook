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

## Completed RC1 validation

The checked source commit was a068c7e8ef433fdd400f10e76a470365fbe626f3. Normal merge preserved its tree at 7139838c065404d764f62b246297a52cedb76bc7. Source-only mixed core, native session/cache and Python session runs each completed their own 24-hour gates before publication. These records remain separate from the public-package runs below.

Public core archive SHA-256 is 667af5d4bed4fae9b209bc7edf353016242521357d6e6dbc2711f9d1d1bbfbd6. Public companion archive SHA-256 is 5d5ad94e8cc1bcfc52ab9a48ba77530a946eee9021d8d1a063dfecd3415b5a19.

| Actual registry workload | Own seconds | Verified count |
| --- | ---: | ---: |
| Core acknowledged mutations | 86400.237814 | 189,223 |
| Native session requests | 86400.241225 | 1,321,099 |
| Python session requests | 86401.080305 | 1,627,400 |
| Mapped record checks | 86400.063479 | 138,419,875 |

All four original runs completed with exit 0 and their own duration/count/lifecycle/identity evidence. The mapped reader observed 354 generations and bound the final completed core publisher receipt.

The remote annotated v1.11.2-rc.1 tag object 6427b2781fdbc55c0608022fd60d5eae39f9d1cd resolves to 7139838c065404d764f62b246297a52cedb76bc7. Separate fresh-cache tag installations passed all 69 document tests including eight SQLite and nine HTML tests, the JSONL byte and document export limits, max-f32 restore and prune regressions, the Node.js app suite and the companion demo.

[Tag CI](https://github.com/gay00ung/vecnook/actions/runs/46115482324) verified eight actual registry CLI/library jobs for each package with Rust 1.89 and stable on Linux x64, macOS ARM64, actual Windows 11 ProductType 1 ARM64 NTFS and Windows Server ProductType 3 x64 NTFS. [Native archive CI](https://github.com/gay00ung/vecnook/actions/runs/46115482284) supplied four core and four companion archives with extracted executable receipts.

The official core and paired release gates passed on these exact archives and completed receipts. Every core job ran 69 document tests, including eight SQLite and nine HTML tests, with checkout, packaged and actual registry CLIs. The installed Python 1.11.2rc1 wheel imported/exported SQLite and HTML documents through the actual registry CLI and passed the 512 MiB byte-limit rejection and small JSONL round trip. It restored full-u64 IDs, maximum positive/negative finite f32 and signed-zero bits. The [public RC1 release](https://github.com/gay00ung/vecnook/releases/tag/v1.11.2-rc.1) contains 79 verified assets: all server digests and sizes, independent unauthenticated downloads and 78 SHA256SUMS rows passed.

Stable 1.11.2 promotion changes package/dependency versions, lockfiles and release documentation. It still requires its own normal PR/main CI, candidate-equivalence gates, core-first publication, real companion Cargo verification against that public core, registry/tag installations, docs.rs, wheel/native assets and final public-download audit. Candidate completion alone does not approve a stable publication.
