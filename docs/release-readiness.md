# Release candidate validation

The 1.x releases preserve the public application API, CLI JSON v1 and storage/model
contracts in [compatibility.md](compatibility.md). Runtime changes after a candidate
require another RC and repeated affected validation. Stable publication is a
separate step with additional duration and desktop-system requirements.

Technical checks include MSRV/current stable on Linux, macOS and Windows, installed
registry consumers, document updates/restores, actual legacy fixtures, fault-point
recovery, read-only diagnosis, measured operating range and package privacy. After
registry publication, CI on the matching `vVERSION` tag installs that exact version
on Linux/macOS/Windows Server with both toolchains and additionally on Windows 11
ARM64 with both toolchains. A manual dispatch with `registry_version` can repeat
the flow when the workflow is available on the default branch. Registry checks use
a fresh Cargo cache for CLI installation and library resolution, compare both Cargo
checksums to an independent archive download, then record the runtime/test fingerprint.
Shared CI timings do not establish performance limits.

Windows Server receipts do not prove Windows 11 compatibility. The additional
`windows-11-arm` jobs exercise GitHub's Windows 11 desktop image. Their actual OS
caption, product type, compiler host and NTFS filesystem are recorded; the label
alone does not pass the gate. This validates a hosted ARM64 environment, rather
than every physical desktop configuration. See [GitHub's runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).

To repeat the same distribution flow on another Windows 11 NTFS machine with
Python 3.12+, Rust and Node.js available:

```bash
python tools/check_distribution.py --registry 1.5.0-rc.2 --toolchain stable --output work/windows11-registry
```

The receipt records the actual OS caption, product type and filesystem. Server CI
does not supply the required Windows 11 evidence.

Run the exact candidate archive's mixed workload separately from benchmarks:

```bash
python3 tools/run_soak.py --crate work/registry/vecnook-1.5.0-rc.2.crate --output work/soak-rc2 --seconds 86400 --mutations 100000
```

`case/progress.json` records actual elapsed time and acknowledgements. The final
receipt completes after ACK replay, exact results, restarts and independent backups
pass. A short 100k run or a running/interrupted workload leaves the duration gate
pending. A partly measured run cannot be silently resumed; use a new directory.

`tools/check_release.py` takes `--candidate-crate`, repeated `--distribution` paths
for all six CI pairs and Windows 11, and `--soak`. Include `--release-crate` for the
prepared stable archive. Runtime source, tests, tools and original build settings
must match the RC; the package version and documentation can change for stable
release. Keep corresponding CI runs and local logs. Receipts are operational
evidence, rather than cryptographic proof of remote execution.

Store checked receipts as `distribution-*.json` and `soak-receipt.json` on the RC's
GitHub release. The stable `Publish` workflow downloads these and the registry
archive, compares evidence and prepared stable source, then permits upload.
Missing, incomplete or mismatched evidence stops publication. A prerelease may be
published while stable-only checks remain pending; release notes show their status.

Local publication uses the same `check_release.py` gate with the prepared stable
archive before `cargo publish --locked`. Existing Cargo credentials remain in the
local credential store. The 1.1 validation record includes the completed registered
candidate workload and its attached receipts in [validation-1.1.md](validation-1.1.md).

External first-run observations and real apps' repeat usage are adoption goals.
Repository examples do not count as external users. Record that evidence separately
through the [feedback forms](feedback.md). These observations are not a release
approval requirement. The 1.2 [memory/filter measurements and completed registry QA](validation-1.2.md)
are recorded separately from the completed 1.1 run.
The [1.3 validation record](validation-1.3.md) includes the completed RC2 registry
workload and distribution checks, separate from unmeasured external-user observations.

## Paired 1.4 engine gates

The 1.4 core and optional platform companion require **new** registry archives,
source fingerprints and receipts. Older release evidence never supplies their
approval. All eight core receipts must exercise segmented generations. Eight
additional companion registry receipts must prove both exact archives on Linux,
macOS, Windows Server NTFS and actual Windows 11 ARM64 NTFS, with MSRV and stable
on each. `check_acceleration.py --registry ADAPTER --core-registry CORE` uses
independent downloads, fresh Cargo caches, installed CLI/platform tests and an
external library consumer run twice.

Run the checked core archive with `run_soak.py --seconds 86400 --mutations 100000`.
Its new workload checks generation publication, retained old views, SQ8 reranking
and independent generation backup alongside the existing mixed mutation/ACK
checks. Once its running receipt exists, start `run_acceleration_soak.py` with
`--core-crate`, `--adapter-crate`, `--publisher-receipt`, `--output`, `--seconds
86400 --records 100000`. This separate process maps the live publisher's immutable
generations, compares originals/SIMD/SQ8 and records observed generation changes.
Its own elapsed duration must reach 24 hours. It binds the final successful core
publisher receipt, rather than a running snapshot. Short or interrupted runs are
diagnostic evidence only and remain preserved.

Attach `acceleration-distribution-*.json` and a completed
`acceleration-soak-receipt.json` alongside the core distributions/soak on the core
candidate's GitHub release. `check_engine_release.py` requires both candidate
archives, both sets of repeated distribution paths and both soak receipts. It
rejects mismatched package/source hashes, missing OS/toolchain hosts, short runs,
missing generation checks and a different publisher receipt. Promotion additionally
compares `--release-crate` and `--adapter-release-crate`: only package/docs versions
and the narrowly verified exact paired core dependency/checksum may differ.

The stable companion's exact stable core dependency is unavailable until core
publication. `prepare_adapter_release.py` therefore prepares a **review-only**
version-adjusted companion archive from the checked candidate and prepared stable
core, verifies both runtime fingerprints, and permits the paired prepublication
gate. Never upload that staged archive as an actual Cargo package. After publishing
the gated stable core, run Cargo packaging/verification for the real stable
companion and rerun the paired gate with that actual archive before its publication.
Recheck both public registry downloads, installed consumers and tag CI afterward.

Native CLI CI produces four core ZIPs and four companion ZIPs, each with its own
SHA-256 and extracted-executable receipt. Publish these, both exact `.crate`
archives/checksums, the Python wheel/checksum and the paired QA receipts. Verify
public asset download hashes/server digests. A green binary build alone does not
approve the registered source or full-duration workload. See
[1.4 validation](validation-1.4.md) for the current measured status.

## Additional 1.5 cache/session gate

The [1.5 candidate protocol](https://github.com/gay00ung/vecnook/blob/v1.7.0/docs/qa-1.5.md) adds prepared-cache/lease checks to the
core workload and an independent full24h session/cache workload. Stable approval
requires those exact-archive lifecycle/request/fixture gates alongside the paired
core/mapped-reader gates. New packaged tools change the frozen source fingerprint,
even when search runtime is unchanged. This RC2 also includes the accepted HNSW
distance-scoring change and needs fresh candidate validation; RC1 receipts cannot
approve its changed runtime or tool sources.

The checked 1.5 RC2 completed these candidate gates, including the separate
actual-registry session/cache workload and all public assets; exact identities and
results are in [1.5 candidate QA](https://github.com/gay00ung/vecnook/blob/v1.7.0/docs/qa-1.5.md). Stable 1.5 promotion preserves the
checked runtime/tests/tools/build settings and still requires the normal PR/main
CI, official publication/equivalence gates and fresh actual stable distributions.

## 1.6 Python session candidate

The additive Python retained-session client changes the packaged source identity.
The checked core 1.6.0-rc.2/Python 1.6.0rc2 and companion 0.1.2-rc.2 completed
the source and registry gates, including four independent registry 24-hour
workloads. [1.6 candidate QA](https://github.com/gay00ung/vecnook/blob/v1.7.0/docs/qa-1.6.md) records the results and the separate
requirements for stable publication.

## 1.7 Node.js session candidate

The checked 1.7.0 RC1 completed separate source and registry gates, including
four independent registry 24-hour workloads and the packaged Node.js bridge
against the registry CLI on all eight core distribution jobs. Stable promotion
requires the separate publication and installation gates. [1.7 candidate QA](https://github.com/gay00ung/vecnook/blob/v1.7.0/docs/qa-1.7.md)

## 1.8 Node.js local app candidate

The packaged app example and test change the core source fingerprint. The
[checked 1.8 RC1](https://github.com/gay00ung/vecnook/releases/tag/v1.8.0-rc.1) completed separate source and registry
checks, four independent 24-hour registry workloads, and eight actual registry
jobs per crate. Each core job ran the Node.js suite with the matching CLI.
Stable promotion requires separate publication and installation checks.
[1.8 candidate QA](https://github.com/gay00ung/vecnook/blob/v1.8.0/docs/qa-1.8.md)

## 1.8.1 document sync prune candidate

Pruning is limited to the selected file formats. The embedded document client
and its regression test change the shipped source fingerprint. Verify the
actual installed CLI's dry-run and write paths against a mixed Markdown/text
folder. The [1.8.1 candidate QA](https://github.com/gay00ung/vecnook/blob/6165904d73b1d5925385db1bc459cef01ff7a1c9/docs/qa-1.8.1.md)
requires fresh source and exact registry evidence for both archives.

## 1.9 Python text sync candidate

Python `Client.sync` exposes the existing CLI text scan as an opt-in keyword.
The packaged wheel source and integration test change the core fingerprint.
Verify preview, scan and selected-format pruning through the installed CLI.
[1.9 candidate QA](https://github.com/gay00ung/vecnook/blob/e1e703c8f935f9dc22bec6922bf8e35762136c0c/docs/qa-1.9.md)
specifies fresh source, exact registry and stable promotion gates.

## 1.10 static HTML input candidate

The optional standard-library HTML example uses existing JSONL v1 and caller-owned
embeddings. Its packaged parser and tests create a new source fingerprint. Run its
parser/argument bounds and installed-CLI import/export/re-import from every source
and actual registry distribution. [1.10 candidate QA](https://github.com/gay00ung/vecnook/blob/f076fcbfab167fc2f563ec0b0997182b15990a1e/docs/qa-1.10.md) requires fresh independent
source and registry workloads and all paired/native/stable gates.

## 1.9.1 JSONL restore candidate

Restore a JSONL archive exported by the native CLI at the finite f32 limits.
The embedded document script and its regression test change the shipped source
fingerprint. [1.9.1 candidate QA](https://github.com/gay00ung/vecnook/blob/8eab5c90f2aab19a03e3364e68c9304683c736e0/docs/qa-1.9.1.md) records the source and registry gates.

## 1.11 SQLite input candidate

The optional read-only SQLite example and its regressions change the packaged
source identity. Each source and actual registry core distribution must run the
SQLite suite with its installed CLI, including JOIN/WAL snapshot consistency,
refused writes, bounded validation, no output overwrite, full-u64/original-text and
f32/signed-zero round trips. The new example needs Python 3.11+/SQLite 3.22+;
existing Python client/CLI compatibility remains unchanged. The
[1.11 candidate protocol](https://github.com/gay00ung/vecnook/blob/20344cb7c5976969017cd1c20fa3f4e3eeea9409/docs/qa-1.11.md)
requires new independent source and exact registry workloads, paired archives,
wheel/native/tag consumers and separate stable publication gates.

## 1.11.1 JSONL export candidate

The embedded document CLI checks the JSONL restore count limit before
creating an export. The packaged script and its boundary regression change
the candidate identity. Run the actual CLI test on the fresh source,
packaged and registry distributions. [1.11.1 candidate QA](qa-1.11.1.md)
records the source, registry, tag, native and stable release gates.

The 1.11.1 RC1 verification covers the 24-hour source and registry runs, platform receipts and independent public asset downloads. [Checked 1.11.1 RC1](https://github.com/gay00ung/vecnook/releases/tag/v1.11.1-rc.1)

## 1.11.2 JSONL byte limit candidate

The shared byte cap and regression change the runtime and test fingerprints.
Run [1.11.2 candidate QA](qa-1.11.2.md) for the exact fresh source and registry
archives, then the paired publication, wheel, native and tag gates. Verify the
UTF-8 boundary and the actual CLI with the 9,000-document fixture.

The [checked 1.11.2 RC1](https://github.com/gay00ung/vecnook/releases/tag/v1.11.2-rc.1) records the 24-hour source and registry runs, platform receipts and independent public asset verification.

## 1.11.3 Markdown fence candidate

The optional `markdown-v1` chunker keeps a short fenced block together when
its contents include a different or shorter marker. Run the installed CLI
regression from [1.11.3 candidate QA](qa-1.11.3.md) on checkout, packaged and
registry builds. The changed runtime script and regression require fresh
source and registry workloads before promotion.
