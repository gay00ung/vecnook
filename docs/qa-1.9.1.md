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
