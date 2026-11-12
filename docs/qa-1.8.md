# 1.8 candidate QA

Core `1.8.0-rc.1` packages a runnable Node.js snapshot app and its regression test.
The app publishes two fixture records, streams an ID-filtered batch, publishes
a replacement and reopens the reader to see the new generation. Python
`1.8.0rc1` uses the existing client API, and companion `0.1.4-rc.1` pins the exact
candidate core. [Node.js app PR](https://github.com/gay00ung/vecnook/pull/18)

## Source candidate

- Freeze the clean feature commit and Cargo source archive. Record the VCS
  identity, archive digest, source fingerprint and installed CLI digest. Check
  the source package contents, relative documentation links and the four-platform
  CI matrix with Rust 1.89 and stable.
- Run the Node.js 24 test suite from the extracted archive with its installed
  CLI. The app test checks full-range IDs, result order, the pinned old record
  across publication and the replacement returned after reopening.
- Run separate mixed core, native session/cache and Python session workloads.
  Each must reach its own 86,400 seconds and pass the count, fixture, hash and
  lifecycle checks with exit status zero. Record this as source evidence.

## Public candidate

After normal PR merge and clean-main CI, publish the checked core candidate
and verify an independent registry download. Cargo-package and verify the
actual companion against the public core checksum, then run the paired archive
gate before publishing the companion. [Releasing](releasing.md) · [Release readiness](release-readiness.md)

Run four separate 24-hour workloads with the exact registry pair. These cover
mixed core, native session/cache, Python session and the mapped reader. The
mapped reader binds the final core publisher receipt. Check the eight registry
distribution receipts per crate, including Windows 11 ARM64 ProductType 1 and
Windows Server x64 ProductType 3 on NTFS with both Rust toolchains. The core
receipts must exercise the packaged Node.js suite with the matching CLI.

Check the wheel with the registry CLI, docs.rs, independent annotated tag
installs, tag CI, native ZIPs and every public asset with `SHA256SUMS`. Attach
the exact evidence to the candidate prerelease. [Paired release gates](release-readiness.md#paired-14-engine-gates)

## Stable promotion

Promote the checked source with version, exact dependency, lockfile and
documentation edits. Use separate clean-main and tag CI, the ordered core and
actual companion publication gates, fresh registry consumers and the formal
release verification. Repository examples and external app observations have
separate evidence. [Feedback workflow](feedback.md)

## RC1 evidence

The [public prerelease](https://github.com/gay00ung/vecnook/releases/tag/v1.8.0-rc.1) records clean-main
commit 520849294c8c9c5a26e9ef72d72e0ff4406d62bf and the annotated tag. These are the actual Cargo registry archives and the
Python wheel checked against the registry CLI:

| Archive | SHA-256 |
| --- | --- |
| vecnook-1.8.0-rc.1.crate | 4431452ffc2c40d08f3182d3db61e5eb50140ef9d9859d211850325fcfcb6fd3 |
| vecnook-accel-0.1.4-rc.1.crate | 50095b44f1b990f394110d4ca75a63d94ffd72e58c0a589a894beda829ae80c3 |
| vecnook-1.8.0rc1-py3-none-any.whl | 0bc9fb50390920ec2d54c9a46a99b9717cc687fd072feea884c261fdd39d46cc |

The separate source-only audit used commit 2513892b80b76cb7773dcec50ae1fccdb5d526e0.
Each of the four public registry runs completed with exit status zero and
at least 86,400 seconds on its own clock.

| Workload | Elapsed seconds | Checked result |
| --- | ---: | --- |
| Mixed core | 86400.578369833 | 189,289 acknowledged mutations, 17 verified backups |
| Native sessions | 86400.19129700004 | 1,269,865 verified requests, 311 released leases |
| Python sessions | 86400.088853875 | 1,620,059 verified requests, 396 released leases |
| Mapped reader | 86400.044292958 | 138,136,021 checked records, 355 observed generations |

The mapped reader binds the successful final core publisher receipt.
[Tag CI](https://github.com/gay00ung/vecnook/actions/runs/43138437679) checked both exact registry
packages and the packaged Node.js suite across Linux x64, macOS ARM64,
Windows Server x64 NTFS and Windows 11 ARM64 NTFS with Rust 1.89 and stable.
Both remote tag installations, the wheel against the registry CLI and the
[eight native ZIPs](https://github.com/gay00ung/vecnook/actions/runs/43138437667) passed. All
74 assets matched server sizes and digests and independent
unauthenticated downloads. SHA256SUMS covers the other 73
assets.

These counts describe the release workloads. Independent observations from
external users and real applications are tracked separately.
