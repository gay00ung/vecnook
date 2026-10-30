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

The [1.5 candidate protocol](qa-1.5.md) adds prepared-cache/lease checks to the
core workload and an independent full24h session/cache workload. Stable approval
requires those exact-archive lifecycle/request/fixture gates alongside the paired
core/mapped-reader gates. New packaged tools change the frozen source fingerprint,
even when search runtime is unchanged. This RC2 also includes the accepted HNSW
distance-scoring change and needs fresh candidate validation; RC1 receipts cannot
approve its changed runtime or tool sources.
