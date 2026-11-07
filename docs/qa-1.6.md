# 1.6 candidate QA

Core 1.6.0-rc.2, Python 1.6.0rc2 and exact-core companion 0.1.2-rc.2 completed
candidate validation. The [public RC2 release](https://github.com/gay00ung/vecnook/releases/tag/v1.6.0-rc.2)
contains the checked files and receipts. Stable 1.6.0/0.1.2 has a separate
publication and installation gate.

## Verified RC2 evidence

The feature archive had SHA-256
`b5a78f85fe0b0c378469971ef77db6e0bda6dde2956aead71138143b45b7fc7e`.
Its three independent source-only runs completed before
[PR #13](https://github.com/gay00ung/vecnook/pull/13) merged. Those runs verified
189,194 core mutations, 1,287,868 native requests and 1,630,637 Python requests.

The registered core and companion were built from checked main
`2a61d0de129f2d934de6ac86ddd4055e144c6c6f`. Their archive SHA-256 values are
`47a9ef82c2ddd80840d9bf598ffa51ad0a6e424199039e857d64cec6c1358f23`
and `11edbf9d2f3a2fe9e253080066765568b57de47d060c4423419e3ff3ee4f4d29`.
Each registry workload completed on its own clock.

| Workload | Elapsed seconds | Verified result |
| --- | ---: | --- |
| Core mixed operations | 86,400.571 | 189,146 acknowledged mutations and 17 verified backups |
| Native sessions | 86,400.181 | 1,257,189 requests and 307 released cache leases |
| Python sessions | 86,401.118 | 1,612,755 requests and 394 released cache leases |
| Mapped reader | 86,400.079 | 137,712,726 checked records and 352 generations |

The mapped reader binds the final core receipt. Both session workloads released
all leases and recorded zero session re-encodings. The
[tag CI](https://github.com/gay00ung/vecnook/actions/runs/42340767410) supplied
eight registry receipts per crate on Linux, macOS, Windows Server x64 NTFS and
Windows 11 ARM64 NTFS with Rust 1.89 and stable. The official paired gate and
both remote tag installations passed. The Python wheel passed with the
registry CLI. Exact docs.rs API pages, all eight native ZIPs and all 73 public
assets passed verification, including independent downloads and `SHA256SUMS`.
These counts describe the validation workload.

## Local RC1 preparation history

RC1 was packaged locally before a new inner-product edge check found that the
Python decoder lost a distance sign when native JSON used `-0`. RC2 preserves
that f64 sign and adds a real native regression assertion. The unpublished RC1
archive and diagnostic are retained; no RC1 full-duration run or registry
publication started. RC2 uses a new exact archive/source identity.

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
