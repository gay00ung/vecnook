# 1.8.1 candidate QA

In 1.8.0, a folder first indexed with `--include-text` can lose its stored
text records when a later Markdown-only `docs-sync --prune` runs, even if the
text files still exist. This patch restricts pruning to the formats selected
for the current scan. Explicit text pruning, namespace ownership, atomic
source updates, original UTF-8 and f32 data, JSON v1 and storage are unchanged.

The candidate is core `1.8.1-rc.1`, Python `1.8.1rc1` and companion
`0.1.5-rc.1`. The companion pins this exact core and retains full-segment mmap.
The embedded client and tests are packaged, so 1.8.0 receipts do not approve
these archives.

## Source candidate

- Freeze a clean feature commit and its Cargo archive. Record the commit,
  tree, archive hash, normalized source fingerprint and installed CLI hash.
  Verify the archive contents, docs links and normal PR CI, including the
  source-only and native artifact matrices.
- Run the embedded client regression against the CLI installed from that
  archive. The test indexes a Markdown file and an uppercase `.TXT` file,
  then checks that Markdown-only dry-run and actual pruning remove only the
  missing Markdown source and preserve the text record. It also confirms
  that explicit text pruning removes the missing text source.
- Run separate mixed core, native snapshot session/cache and Python snapshot
  session workloads from the frozen source. Each must exit zero with at least
  86,400 seconds on its own clock and pass its count, fixture, hash and lifecycle
  checks. Record these as source-only evidence.

## Public candidate and stable promotion

Follow [releasing](releasing.md) and [release readiness](release-readiness.md).
After normal merge and clean-main CI, publish and independently download the
exact core, verify the actual Cargo-packaged companion against that public core
checksum, pass the paired archive gate and publish the companion. Run four
new independent 24-hour registry workloads: mixed core, native sessions/cache,
Python sessions and the mapped reader bound to the final core publisher receipt.

Check eight actual registry consumers per crate, including Windows 11 ARM64
ProductType 1 and Windows Server x64 ProductType 3 on NTFS with Rust 1.89 and
stable. Keep the source-only, registry and native receipts distinct. Check
fresh CLI/library installs, docs.rs, the wheel with the registry CLI, annotated
remote tag installs, tag CI, native ZIPs and independent downloads of all
release assets against server digests and SHA256SUMS. Promote only the checked
runtime, tests, tools and build settings. Stable has its own PR, clean-main,
paired publication and public verification gates. Internal fixtures do not
establish external adoption.
