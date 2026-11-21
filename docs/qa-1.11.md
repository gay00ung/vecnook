# 1.11 SQLite input candidate QA

The candidate adds an optional standard-library read-only SQLite SELECT/JOIN
example that emits existing JSONL v1 from app-owned vectors/model identity.
It defines nine column aliases and bounded validation/output semantics without
adding a SQL engine, embedding provider, core data model or storage format.
The new example needs Python 3.11+ and SQLite 3.22+ for runtime connection limits.
Existing Python client/document CLI support remains Python 3.10+.

Planned versions are core `1.11.0-rc.1`, Python `1.11.0rc1` and exact paired
companion `0.1.9-rc.1`. The packaged script and regressions change the frozen
source fingerprint. Older 1.10 receipts and the isolated prototype do not approve
this candidate. These requirements remain pending until exact receipts complete.

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
