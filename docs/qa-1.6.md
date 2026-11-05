# 1.6 candidate QA

Core 1.6.0-rc.1, Python 1.6.0rc1 and exact-core companion 0.1.2-rc.1 are in
preparation. No 1.6 candidate publication, registry workload or stable promotion
is approved by this document. The added Python client/tests change the packaged
source fingerprint even though the Rust engine and native protocol are unchanged.
Earlier 1.5/RC2/RC1 identities, clocks and measurements remain historical evidence.

## Scope and focused checks

`SnapshotSession` is an additive standard-library Python API for an existing
immutable generation store. It reuses one native process, pins one generation,
streams batches and explicitly opts into resident SQ8 or prepared caches.
Existing mutable `Client` methods keep their per-call behavior. The core remains
dependency-free, forbids unsafe code, supports Rust 1.89 and preserves original
records/f32, JSON v1, default searches and storage formats. The optional companion
keeps full-segment mmap; bounded-window mmap is excluded.

`examples/documents/test_snapshot_client.py` exercises real subprocess startup,
query-error recovery, malformed/oversized responses, read/blocked-write timeouts,
concurrent callers, streaming partial results, deterministic close and garbage
collection fallback. Native checks cover original UTF-8/full-u64/signed-zero,
old-reader pinning across `CURRENT` changes, generation GC, prepared-cache lease
refusal/release and idle exit. CI discovers these tests on each existing
OS/toolchain job. Compatibility checks against the published 1.5 CLI are separate
from tests of the new exact source/registry candidate.

## Required new release evidence

Follow [releasing](releasing.md) and [release readiness](release-readiness.md).
Freeze one clean feature archive and record its VCS/source/archive/binary hashes.
Run new source-only mixed and native session/cache workloads, each with its own
actual 86,400-second clock and the existing count/lifecycle/fixture gates. Also
exercise this archive's Python session API across repeated close/reopen, native
query errors, process exit and cache leases; record that client workload separately
from the native JSONL workload. A running receipt does not approve publication.

After normal PR merge and clean-main CI, package/verify and publish the exact
candidate core. Independently download it before packaging the companion against
that public exact core/checksum. Publish only the actual Cargo-verified companion
archive. Prepare-adapter staged archives remain review-only.

The exact registered pair needs new core, independent mapped-reader and native
session/cache full24h workloads under the inherited [lifecycle/count gates](qa-1.5.md#independent-24-hour-workloads).
Each uses its own elapsed time and exact hashes; source-only, old releases,
partial runs and elapsed-time sums cannot substitute. The mapped reader binds the
final successful publisher receipt. Preserve failed/short diagnostics.

Require eight actual-registry core and eight paired companion receipts on
Linux/macOS/Windows Server x64 NTFS/actual Windows 11 ARM64 NTFS, each with MSRV
and stable. Inspect actual caption/ProductType/architecture/filesystem, both
`platform_errors` lists, normal annotated remote-tag installations and tag CI.
Run the official `check_engine_release.py` paired gate with all 16 receipts and
the exact final core/mapped receipts, plus the separate session and Python checks.

Build/install the Python wheel against the actual registry CLI and exercise the
new API. Check exact docs.rs versions/APIs, eight tag-built native ZIPs and their
internal executable hashes. Attach both actual crates, the wheel, distributions,
native files and clearly distinguished source/registry evidence to the public
prerelease. Verify every server digest/size, independent unauthenticated download
SHA and the complete SHA256SUMS list.

Only then prepare a version/dependency/lockfile/documentation-only stable promotion
to core/Python 1.6.0 and companion 0.1.2. Pass official publication/source-equivalence
gates, normal PR/clean-main CI, actual stable core publication/download, actual
companion Cargo packaging against it and the paired gate again before companion
publication. Fresh stable registry consumers, wheel, remote tag/tag CI/native
ZIPs, exact docs.rs and a public formal release with all assets verified remain
required. External users/real-app observations remain unmeasured.
