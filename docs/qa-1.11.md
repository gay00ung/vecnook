# 1.11 SQLite input candidate QA

The candidate adds an optional standard-library read-only SQLite SELECT/JOIN
example that emits existing JSONL v1 from app-owned vectors/model identity.
It defines nine column aliases and bounded validation/output semantics without
adding a SQL engine, embedding provider, core data model or storage format.
The new example needs Python 3.11+ and SQLite 3.22+ for runtime connection limits.
Existing Python client/document CLI support remains Python 3.10+.

Planned versions are core `1.11.0-rc.2`, Python `1.11.0rc2` and exact paired
companion `0.1.9-rc.2`. The packaged script and regressions change the frozen
source fingerprint. Older 1.10 receipts and the isolated prototype do not approve
this candidate. These requirements remain pending until exact receipts complete.

## RC1 rejection and RC2 identity

RC1 head `4c4fbde603ad0e638bb1ceb2ce10580edf93b4b1` passed local archive checks,
but POSIX PR jobs could not launch the integration test's relative CLI path after
switching to a temporary directory. Windows jobs and native archive builds passed.
RC2 resolves that test path before the cwd change. This changes shipped test
identity and requires a new archive plus all source/registry duration gates.
No RC1 full-duration run was started and no RC1 receipt approves RC2.

## Source candidate

Freeze a clean feature commit, package with Rust 1.89, inspect contents and the
runtime/test/tool/build fingerprint, then install the exact archive in a fresh
cache. Run every document suite, including eight SQLite tests and nine HTML tests,
from that archive against its installed CLI. SQLite checks include read-only JOIN
and committed WAL snapshot behavior, refused mutation/ATTACH/PRAGMA/extensions,
SQL/SQLite-row/result-row/output bounds, invalid schema/metadata/vector inputs,
canonical/full-u64 IDs, original UTF-8/line endings and maximum finite f32/signed
zero (including JSON `-0`) through CLI import/export. Check the standalone script
with minimum Python 3.11 as well as the distribution's Python version.

Run all existing Rust/document/Python/Node/package/release-tool checks. Normal
attached PR source/native CI must pass on the frozen head. Start three independent
full source workloads: core mixed mutations, native session/cache and Python
session. Each needs exit 0, its own at least 86,400 seconds and 100,000 checked
mutations/requests plus current fixture/hash/lifecycle gates. A short or running
receipt is not approval. Do not restart healthy runs or combine durations.

## Exact registry candidate

Follow releasing.md and release-readiness.md. After normal merge and clean-main
CI, publish and independently download the exact core archive. Cargo-package and
verify the real companion against that exact public core/checksum, pass the
actual archive paired gate, then publish the companion. Run four new independent
registry workloads: core, native session/cache, Python session and mapped readers.
Each needs its own full duration/count/exit/lifecycle/identity conditions. The
mapped reader must bind the final successful core publisher receipt.

Verify eight actual registry consumers per crate across Linux x64, macOS ARM64,
actual Windows 11 ProductType 1 ARM64 NTFS and Server ProductType 3 x64 NTFS, with
Rust 1.89 and stable. Every core distribution must execute the packaged SQLite
suite with that installed CLI. Verify fresh CLI/library caches, isolated wheel
against the exact registry CLI, docs.rs, both remote annotated-tag installs,
exact tag CI and eight native archives. Check release flags, all server asset
sizes/digests, independent unauthenticated downloads and SHA256SUMS.

## Stable promotion

A separate normal PR/main CI and official unchanged-source/paired gates must
approve actual stable archives. Publish stable core first; only then Cargo-package
and verify the real stable companion against that public core/checksum and rerun
the actual paired gate before companion publication. Never post a REVIEW ONLY
staged archive. Verify actual stable consumers/wheel/docs/tag/native/public assets
before recording completion. Keep RC source and registry evidence separate.

Historical performance measurements retain their original versions/conditions.
Repository fixtures and this adapter do not establish external-user demand or
actual app adoption. PDF, broad payload models and bounded-window mmap require
separate evidence and specifications.

## Completed RC2 validation

The checked source commit was d30436a3c80e1809b3d2bb765481f6c6dff8fe70. Normal merge preserved its tree at 20344cb7c5976969017cd1c20fa3f4e3eeea9409. Source-only mixed core, native session/cache and Python session runs each completed their own 24-hour gates before publication. These records remain separate from the public-package runs below.

Exact public core archive SHA-256: 75e3033b6211f0baaad0b42916a18bcd5b487a358169d9409cb1db6d0f3647d5. Exact companion archive SHA-256: 0fee51ee69ce288fa770b0c69f3e7ec5fe6e6290c9faec29b4f2e23a9b55cd1b.

| Actual registry workload | Own seconds | Verified count |
| --- | ---: | ---: |
| Core acknowledged mutations | 86400.956832 | 189,103 |
| Native session requests | 86405.234323 | 1,159,408 |
| Python session requests | 86400.103855 | 1,640,156 |
| Mapped record checks | 86400.022501 | 138,973,737 |

All four original runs completed with exit 0 and their own duration/count/lifecycle/identity evidence. The mapped reader observed 355 generations and bound the final completed core publisher receipt.

The remote annotated v1.11.0-rc.2 tag object 4fb09e3a23e3b4e8862141e324314cc9197d9c58 resolves to 20344cb7c5976969017cd1c20fa3f4e3eeea9409. Separate fresh-cache tag installations passed all 67 document tests including eight SQLite and nine HTML tests, the max-f32 JSONL restore and prune regressions, the Node.js app suite and the companion demo.

[Tag CI](https://github.com/gay00ung/vecnook/actions/runs/45298331215) verified eight actual registry CLI/library jobs for each package with Rust 1.89 and stable on Linux x64, macOS ARM64, actual Windows 11 ProductType 1 ARM64 NTFS and Windows Server ProductType 3 x64 NTFS. [Native archive CI](https://github.com/gay00ung/vecnook/actions/runs/45298331460) supplied four core and four companion archives with extracted executable receipts.

The official core and paired release gates passed on these exact archives and completed receipts. Every core job ran 67 document tests, including eight SQLite and nine HTML tests, with checkout, packaged and actual registry CLIs. The installed Python 1.11.0rc2 wheel imported/exported SQLite and HTML documents through the actual registry CLI, preserving full-u64 IDs, maximum positive/negative finite f32 and signed-zero bits. The [public RC2 release](https://github.com/gay00ung/vecnook/releases/tag/v1.11.0-rc.2) contains 78 verified assets: all server digests and sizes, independent unauthenticated downloads and 77 SHA256SUMS rows passed.

Stable 1.11.0 promotion changes package/dependency versions, lockfiles and release documentation. It still requires its own normal PR/main CI, candidate-equivalence gates, core-first publication, real companion Cargo verification against that public core, registry/tag installations, docs.rs, wheel/native assets and final public-download audit. Candidate completion alone does not approve a stable publication.
