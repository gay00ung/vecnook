# Release candidate validation

The 1.0 candidate freezes the public application API, CLI JSON v1 and storage/model
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
python tools/check_distribution.py --registry 1.0.0-rc.1 --toolchain stable --output work/windows11-registry
```

The receipt records the actual OS caption, product type and filesystem. Server CI
does not supply the required Windows 11 evidence.

Run the exact candidate archive's mixed workload separately from benchmarks:

```bash
python3 tools/run_soak.py --crate work/registry/vecnook-1.0.0-rc.1.crate --output work/soak-rc1 --seconds 86400 --mutations 100000
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

External first-run observations and real apps' repeat usage are adoption goals.
Repository examples do not count as external users. Record that evidence separately
through the [feedback forms](feedback.md).
