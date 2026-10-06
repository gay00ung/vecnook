# Release a checked source package

Vecnook is currently distributed from GitHub. crates.io publication requires a verified registry account and publishing credentials. GitHub push access alone does not provide those credentials. Do not advertise a registry version until its public registry entry has been verified.

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

Follow the [Cargo publishing instructions](https://doc.rust-lang.org/cargo/reference/publishing.html) to sign in, verify the registry account, and create a narrowly scoped token for this crate. Use `cargo login` locally, or store the token in the repository's `CARGO_REGISTRY_TOKEN` Actions secret. Keep it out of source, command-line arguments and logs.

The `Publish` workflow uses `workflow_dispatch`. Its default mode verifies the package without uploading it. Select `publish=true` on the intended commit only after checking CI and credentials. The publishing step refuses an empty credential and uses Cargo's environment credential provider. It is never run for a pull request.

After publication, verify the version at `https://crates.io/crates/vecnook` and the build at `https://docs.rs/vecnook`. Test a fresh `cargo add vecnook@VERSION` and a small runnable consumer. Only then change the installation instructions. Registry versions cannot be overwritten; see [Cargo's permanence rules](https://doc.rust-lang.org/cargo/reference/publishing.html).

## GitHub source release

Create an annotated `vVERSION` tag on the checked commit and push it. Attach the `.crate` source archive and its SHA-256 digest to a GitHub prerelease. Include the version, supported systems, storage compatibility, new behavior, validation and measured limits. Install once from the actual remote tag, independently of the working tree.
