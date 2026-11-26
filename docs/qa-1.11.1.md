# 1.11.1 candidate QA

The 1.11.0 CLI writes a JSONL export for 10,001 active documents. The
JSONL importer accepts at most 10,000 and refuses that export. Native binary
transfer restores all 10,001 documents. This patch applies the existing
count limit before creating a JSONL file. The native transfer remains
available for larger collections.

The candidate pairs core `1.11.1-rc.1`, Python wheel `1.11.1rc1` and
companion `0.1.10-rc.1`. The companion pins the exact core and uses
full-segment mmap.

## Source candidate

- Freeze a clean feature commit and package with Rust 1.89. Check the
  packaged contents and independent source fingerprints.
- Run the actual CLI regression on checkout and packaged distributions.
  Compare all records after restoring exactly 10,000 documents. At 10,001,
  require a nonzero export status, absent output and intact source records.
  Keep the existing f32, prune, HTML, SQLite and Node app checks.
- Run fresh mixed core, native cache/session and Python session workloads.
  Each needs its own duration of at least 86,400 seconds, exit 0, 100,000
  verified operations, exact artifact hashes and lifecycle evidence.

## Registry candidate and stable promotion

Follow [releasing](releasing.md) and [release readiness](release-readiness.md).
After normal PR CI, a normal merge and clean-main CI, publish the core and
verify the public checksum by independent download. Package and verify the
actual companion against that public core. Pass the actual archive paired
gate before publishing the companion. Staged review adapters serve package
review only.

Run four fresh actual registry 24-hour workloads. The mapped reader binds
the successful completed core publisher receipt. Verify the exact core
and companion CLI and library consumers on Linux x64, macOS ARM64,
Windows 11 ProductType 1 ARM64 NTFS and Windows Server ProductType 3 x64
NTFS, with Rust 1.89 and stable. Check the isolated wheel against the
installed registry CLI, docs.rs exact versions, fresh annotated remote tag
installs, tag CI, real native executables and every public asset digest,
size and SHA256SUMS row by independent unauthenticated download.

Stable promotion requires its own PR and clean-main CI, official
candidate equivalence, core-first paired publication, installations,
wheel and native checks and final public download verification.

## Completed RC1 validation

The checked source commit was 909d174a17866fa2ead21f396761354c15754210. Normal merge preserved its tree at 081602bf77432b66d1e6f733fcb7cd8dd52bbb0a. Source-only mixed core, native session/cache and Python session runs each completed their own 24-hour gates before publication.

Public core archive SHA-256 is d1a2326c316dffef6cfdeb115f7ec20347e3752deb9dce62577dbd6b0d57056d. Public companion archive SHA-256 is 21b5a22a7fe128d8fe3ed2e68cfe8aa01de90a1d2bfc86589f383afd4b15227f.

| Actual registry workload | Own seconds | Verified count |
| --- | ---: | ---: |
| Core acknowledged mutations | 86401.088813 | 189,129 |
| Native session requests | 86405.271306 | 1,277,474 |
| Python session requests | 86400.064394 | 1,621,688 |
| Mapped record checks | 86400.028523 | 138,089,753 |

All four original runs exited with code 0. Each run met its own duration and count thresholds, and its lifecycle and archive identity were verified. The mapped reader observed 348 generations and bound the final completed core publisher receipt.

The remote annotated v1.11.1-rc.1 tag object be2231c9add973343b75990579bf8feaca1c69f3 resolves to 081602bf77432b66d1e6f733fcb7cd8dd52bbb0a. Separate fresh-cache tag installations passed all 68 document tests including eight SQLite and nine HTML tests, the JSONL export limit, max-f32 restore and prune regressions, the Node.js app suite and the companion demo.

[Tag CI](https://github.com/gay00ung/vecnook/actions/runs/45754514755) verified eight actual registry CLI/library jobs for each package with Rust 1.89 and stable on Linux x64, macOS ARM64, actual Windows 11 ProductType 1 ARM64 NTFS and Windows Server ProductType 3 x64 NTFS. [Native archive CI](https://github.com/gay00ung/vecnook/actions/runs/45754514644) supplied four core and four companion archives with extracted executable receipts.

The official core and paired release gates passed on these exact archives and completed receipts. Every core job ran 68 document tests, including eight SQLite and nine HTML tests, with checkout, packaged and actual registry CLIs. The installed Python 1.11.1rc1 wheel imported/exported SQLite and HTML documents through the actual registry CLI and passed the 10,000/10,001-document export boundary check. It restored full-u64 IDs, maximum positive/negative finite f32 and signed-zero bits. All 78 GitHub assets passed independent unauthenticated download and server digest and size checks. The audit also matched 77 rows in SHA256SUMS. [Checked 1.11.1 RC1](https://github.com/gay00ung/vecnook/releases/tag/v1.11.1-rc.1)

Stable 1.11.1 promotion changes package/dependency versions, lockfiles and release documentation. It still requires its own normal PR/main CI, candidate-equivalence gates, core-first publication, real companion Cargo verification against that public core, registry/tag installations, docs.rs, wheel/native assets and final public-download audit.
