# Release a checked source package

Vecnook is distributed as versioned registry packages and GitHub source releases. crates.io publication requires a verified registry account and publishing credentials. GitHub push access alone does not provide those credentials. Prepare the version and installation documentation locally, then verify the public registry entry before pushing or announcing the release.

## Verify the version

Update `Cargo.toml`, regenerate `Cargo.lock`, and record the actual behavior in `CHANGELOG.md`. On the clean release commit:

```bash
cargo fmt --check
cargo test --offline
cargo clippy --offline --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --offline --no-deps
cargo package --offline
cargo package --offline --list
cargo tree --offline
```

CI also checks Rust 1.89. Inspect the source archive: it must contain no `work/`, `data/`, planning/QA documents, Python bytecode, credentials, or generated database files. Install the packaged source into a fresh directory and test a consumer of the packaged library. Runtime model weights and benchmark datasets stay outside the crate.

## Registry authentication

For a stable release, first complete [candidate validation](release-readiness.md).
Run the exact candidate registry package on all six OS/toolchain combinations,
Windows 11 NTFS, and the full 24-hour/100k acknowledged-mutation workload. Upload
the checked receipt JSON to that candidate's GitHub release. The Publish workflow
requires `candidate_version=VERSION-rc.N` and rejects missing or mismatched evidence.
Prerelease publication can proceed with those stable-only gates explicitly pending.

Follow the [Cargo publishing instructions](https://doc.rust-lang.org/cargo/reference/publishing.html) to sign in, verify the registry account, and create a narrowly scoped token for this crate. Use `cargo login` locally, or store the token in the repository's `CARGO_REGISTRY_TOKEN` Actions secret. Keep it out of source, command-line arguments and logs.

The `Publish` workflow uses `workflow_dispatch`. Its default mode verifies the package without uploading it. Select `publish=true` on the intended commit only after checking CI and credentials. The publishing step refuses an empty credential and uses Cargo's environment credential provider. It is never run for a pull request.

Before publication, the source package must contain installation instructions for its own version. Verify the public version at `https://crates.io/crates/vecnook` and the build at `https://docs.rs/vecnook`. Test a fresh `cargo add vecnook@=VERSION`, a small runnable consumer and a registry CLI installation. Only then push and announce the prepared release documentation. Registry versions cannot be overwritten; see [Cargo's permanence rules](https://doc.rust-lang.org/cargo/reference/publishing.html).

## GitHub source release

Create an annotated `vVERSION` tag on the checked commit and push it. Attach the `.crate` source archive and its SHA-256 digest to a GitHub prerelease. Include the version, supported systems, storage compatibility, new behavior, validation and measured limits. Install once from the actual remote tag, independently of the working tree.

Publish the registry version before pushing its tag. Tag CI installs the exact
registry version with a fresh Cargo cache and preserves one distribution receipt
for each OS/toolchain. Inspect the six Linux/macOS/Windows Server receipts and the
additional Windows 11 ARM64 receipts, then attach them to the candidate
release under distinct `distribution-*.json` names. Only attach a completed soak
receipt after the full workload finishes. Keep a failed run's original files/logs
for diagnosis; a running receipt is not release approval.

## Maintain 1.x compatibility

Use patch releases for compatible fixes and minor releases for compatible new
features. Preserve the [Rust/JSON/storage contracts](compatibility.md), including
Rust 1.89 support. A required breaking API/JSON or MSRV change needs a new major
version. A storage-format change also needs a documented migration, actual
old-version fixtures and independent restore checks. Human-readable CLI text and
benchmark timings may improve without changing the machine-readable contract.

Record user reports with version, environment, minimal reproduction, expected
behavior, regression evidence, fix version and user confirmation. Prioritize
acknowledged data loss, incorrect exact results, filter leakage and failed restore
over feature additions. Follow the [feedback workflow](feedback.md); do not count
repository examples or downloads as verified external adoption.
