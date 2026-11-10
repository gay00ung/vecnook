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
