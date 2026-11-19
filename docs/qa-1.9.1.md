# 1.9.1 candidate QA

Vecnook 1.9.0 exports the largest finite positive and negative f32 values using
short decimal representations that round back to the original coordinates.
The JSONL importer compares those decimals against the exact maximum magnitude
and rejects them. The fix accepts the finite rounding interval and preserves
the export format and coordinate bits.

The candidate pairs core `1.9.1-rc.1`, Python `1.9.1rc1` and companion
`0.1.7-rc.1`. The companion pins the exact core and retains full-segment mmap.

## Source candidate

- Freeze the clean feature commit and package with Rust 1.89. Audit package
  contents, source fingerprint, normal PR CI and native archives.
- Run the native CLI regression using its embedded document client. Export and
  restore both maximum finite magnitudes, negative zero, a full-u64 ID and
  original UTF-8. Overflow in either direction must fail before destination
  creation.
- Run the mixed core, native session/cache and Python session workloads
  independently. Each must finish with exit 0, at least 86,400 seconds elapsed
  and its own count, lifecycle, fixture and hash evidence.

## Public candidate and promotion

Follow [releasing](releasing.md) and [release readiness](release-readiness.md).
After normal merge and clean-main CI, verify the published core checksum.
Cargo-package the actual companion against that public core, pass the paired
archive gate and publish the companion. Run four independent fresh 24-hour
registry workloads. The mapped reader binds the completed core publisher receipt.

Verify both packages' CLI and library installs with Rust 1.89 and stable on
Linux x64, macOS ARM64, Windows 11 ProductType 1 ARM64 NTFS and Windows Server
ProductType 3 x64 NTFS. Verify the wheel with the installed registry CLI, versioned
docs.rs, remote annotated tag installs, tag CI, native ZIPs and public downloads.
Keep source, registry and native receipts separate. Stable promotion has its own
PR, clean-main CI, actual paired publication and public asset verification.

## Completed RC1 validation

The checked source commit was d376f970848fcf9a72c8f97445479053982482d4. Normal merge preserved its tree at 8eab5c90f2aab19a03e3364e68c9304683c736e0. Source-only mixed core, native session/cache and Python session runs each completed their own 24-hour gates before publication. These records remain separate from the public-package runs below.

Exact public core archive SHA-256: 6b4b26a20655ee04e3978dc8096ef068b852b575b72568aa689d910c8e6557ba. Exact companion archive SHA-256: 43c52893587f92b6ca1c6ba015bda752e0eb9b2f58109b06664b3b6e286bb797.

| Actual registry workload | Own seconds | Verified count |
| --- | ---: | ---: |
| Core acknowledged mutations | 86400.761715 | 189,308 |
| Native session requests | 86400.268242 | 1,216,429 |
| Python session requests | 86400.083626 | 1,651,625 |
| Mapped record checks | 86400.074398 | 139,043,858 |

All four original runs completed with exit 0 and their own duration/count/lifecycle/identity evidence. The mapped reader observed 354 generations and bound the final completed core publisher receipt.

The remote annotated v1.9.1-rc.1 tag object 2033911deb2271187e1f90932b6586c51dc39e5c resolves to 8eab5c90f2aab19a03e3364e68c9304683c736e0. Separate fresh-cache tag installations passed the max-f32 JSONL restore and prune regressions, the Node.js app suite and the companion demo.

[Tag CI](https://github.com/gay00ung/vecnook/actions/runs/44444213928) verified eight actual registry CLI/library jobs for each package with Rust 1.89 and stable on Linux x64, macOS ARM64, actual Windows 11 ProductType 1 ARM64 NTFS and Windows Server ProductType 3 x64 NTFS. [Native archive CI](https://github.com/gay00ung/vecnook/actions/runs/44444213890) supplied four core and four companion archives with extracted executable receipts.

The official core and paired release gates passed on these exact archives and completed receipts. The installed Python 1.9.1rc1 wheel restored the preserved 1.9.0 JSONL export with exact coordinate bits through the public registry CLI. The [public RC1 release](https://github.com/gay00ung/vecnook/releases/tag/v1.9.1-rc.1) contains 76 verified assets: all server digests and sizes, independent unauthenticated downloads and 75 SHA256SUMS rows passed.

Stable 1.9.1 promotion changes package/dependency versions, lockfiles and release documentation. It still requires its own normal PR/main CI, candidate-equivalence gates, core-first publication, real companion Cargo verification against that public core, registry/tag installations, docs.rs, wheel/native assets and final public-download audit. Candidate completion alone does not approve a stable publication.
