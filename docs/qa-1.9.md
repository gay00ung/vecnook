# 1.9 candidate QA

Python `Client.sync` now accepts the keyword `include_text=True` and forwards
the CLI's `--include-text` option. Default calls scan Markdown. Explicit text
scans include UTF-8 `.txt` files in previews, writes and pruning. The core CLI,
search defaults, original UTF-8 and f32 data, JSON v1 and storage are unchanged.

The candidate is core `1.9.0-rc.1`, Python `1.9.0rc1` and companion
`0.1.6-rc.1`. The companion pins this core and retains full-segment mmap. The
packaged Python module and integration test change the core source fingerprint.

## Source candidate

- Freeze a clean feature commit and its Cargo archive. Check the package contents,
  MSRV 1.89, normalized fingerprint, normal PR CI and native binary receipts.
- With the Python client and installed CLI from that archive, verify Markdown and
  uppercase `.TXT` preview without writes or embedding calls, then sync both.
  Verify Markdown-only pruning in preview and write, retention of original text
  and f32 vectors, and explicit pruning of the missing text source.
- Run the mixed core, native session/cache and Python session source workloads
  independently. Require exit 0, at least 86,400 elapsed seconds per process,
  and the count, fixture identity, byte hashes and lifecycle checks for each.

## Public candidate and stable promotion

Follow [releasing](releasing.md) and [release readiness](release-readiness.md).
After normal merge and clean-main CI, publish and independently download the exact
core. Cargo-package the companion against that core checksum, pass the actual
paired archive gate and publish the companion. Run fresh 24-hour registry
workloads for mixed core, native sessions/cache, Python sessions and the mapped
reader. The mapped reader must bind the completed core publisher receipt.

Verify fresh registry CLI and library consumers for both crates with Rust 1.89
and stable on Linux x64, macOS ARM64, Windows 11 ProductType 1 ARM64 NTFS and
Windows Server ProductType 3 x64 NTFS. Verify the installed wheel with the actual
registry CLI, versioned docs.rs pages, remote annotated tag installs, tag CI,
native ZIPs and all public download sizes and hashes against `SHA256SUMS`.
Keep source-only, registry and native evidence distinct. Stable promotion needs
its own PR CI, clean-main CI, actual paired publication and public verification.

## Verified release candidate

The [1.9.0 RC1 release](https://github.com/gay00ung/vecnook/releases/tag/v1.9.0-rc.1) includes the four completed registry workloads,
eight installed platform/toolchain receipts for each crate, the installed wheel
regression, eight native CLI archives and checksums.
