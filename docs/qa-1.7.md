# 1.7 candidate QA

Core `1.7.0-rc.1` includes the [Node.js snapshot session example](engine-generations.md#nodejs-snapshot-session-example)
and its regression suite. Python `1.7.0rc1` retains the published session API.
Companion `0.1.3-rc.1` pins the exact core candidate. The core storage formats,
JSON v1, search defaults, Rust 1.89 support and companion full-segment mmap
follow the existing contracts. [Node.js implementation](https://github.com/gay00ung/vecnook/pull/15)

## Verified RC1 evidence

The public RC1 contains the exact core and companion archives, Python wheel,
four independent registry 24-hour receipts, and the separate source-only and
tag evidence. [Public RC1 release](https://github.com/gay00ung/vecnook/releases/tag/v1.7.0-rc.1)

The three source-only workloads used feature commit
946de3005116e7f8e07b41a6cc6d8cef4662726b and archive SHA-256
ba1cd3c0014c1d19f2df480e665af775c35b578fe8e5d28889a7b7742ef75069. The public pair came from checked main
e84098a2c58a5c2baa5b593256379a0f683363ed. The core archive SHA-256 is
d83e422d3794561bdd84ce351afe237917b5f889ecc2c7d0da893a1137ef92d4. The companion archive SHA-256 is
81d4447407d14477aae00b019f262ec7826f38c38ca2fb5655d321bca9c10fe8.

| Registry workload | Elapsed seconds | Verified result |
| --- | ---: | --- |
| Mixed core operations | 86,400.779 | 189,260 acknowledged mutations and 17 verified backups |
| Native sessions | 86,400.190 | 1,262,956 requests and 309 released cache leases |
| Python sessions | 86,401.102 | 1,626,352 requests and 398 released cache leases |
| Mapped reader | 86,400.009 | 138,383,328 checked records across 353 generations |

Each workload completed on its own clock. The mapped reader binds the final
core publisher receipt. The tag CI verified eight registry distributions per
crate, including Windows Server x64 NTFS and Windows 11 ARM64 NTFS with Rust
1.89 and stable. The packaged Node.js suite passed in all eight core jobs.
[Tag registry CI](https://github.com/gay00ung/vecnook/actions/runs/42726078245)

Both remote tag installations, the official paired gate, the Python wheel with
the registry CLI and seven docs.rs pages passed. All 73 public assets matched
server digests and independent downloads. The SHA256SUMS list and all eight
extracted native executables also matched. [Native tag CI](https://github.com/gay00ung/vecnook/actions/runs/42726077910)

## Source candidate

- Freeze the clean candidate commit and record the actual Cargo package, VCS,
  source fingerprint, extracted source and binary hashes. Check package
  contents, links and the complete CI matrix.
- Run the Node.js 24 suite from the extracted archive against the CLI built
  from that archive. The suite exercises pinning across a new publication,
  cache lease release, filtered u64 results, UTF-8, f32 signed zero, lazy batch
  ordering and transport failure cleanup.
- Start separate mixed core, native session/cache and Python session runs
  using the frozen source package. Verify their own elapsed 86,400 seconds,
  exit status, counts, original records and lifecycle receipts before approving
  the source candidate. Source-only evidence is kept separately from registry
  and tag evidence.

## Public candidate

Follow [releasing](releasing.md), [release readiness](release-readiness.md)
and the inherited [1.6 release evidence](qa-1.6.md#required-new-release-evidence).
After normal PR merge and clean-main CI, publish the verified core candidate
and independently download it. Verify and package the actual companion against
that exact public core checksum before publishing the companion.

The public pair requires four new independent 24-hour receipts for mixed core,
mapped reader, native session/cache and Python session. The mapped reader binds
the final completed core publisher receipt. Each run must pass its own duration,
counts, fixture, hashes and lifecycle checks. Run the official paired gate with
eight core and eight companion registry receipts on Linux, macOS, Windows
Server x64 NTFS and Windows 11 ARM64 NTFS, each on MSRV and stable.

On the same platform matrix, run the packaged Node.js bridge suite with the
CLI installed from the exact registry version. Each core distribution receipt
records `node_snapshot_session` after this suite passes. Verify the actual
wheel, docs.rs, annotated remote tag
installations, tag CI, native ZIPs, and all independent public asset downloads.
Candidate evidence is recorded on the prerelease with the exact archive hashes.

## Stable promotion

After the candidate gates, use the [paired publication sequence](release-readiness.md#paired-14-engine-gates)
for a version, dependency, lockfile and documentation promotion. Require
separate clean-main and tag CI, actual stable core and companion archives, fresh
registry consumers, the stable wheel and formal GitHub release verification.
Record external user observations separately from package and example checks.
