# 1.5 candidate QA

The checked candidate is core 1.5.0-rc.2, Python 1.5.0rc2 and companion
0.1.1-rc.2 with an exact core dependency. Its actual registry, full-duration,
platform and public-distribution gates are complete. The metadata-only stable
promotion targets core/Python 1.5.0 and companion 0.1.1. P2 bounded-window mmap is excluded.
The candidate includes the accepted HNSW distance-scoring change from main
`b71cf70`, alongside additional packaged benchmark/comparison/demo tools. Its runtime
and frozen source fingerprint differ from RC1 and require fresh source/archive evidence.

## Completed RC2 registry validation — 2026-10-31

[RC2's public prerelease](https://github.com/gay00ung/vecnook/releases/tag/v1.5.0-rc.2)
contains the exact archives, 16 registry distribution receipts, eight native ZIPs, Python
wheel and separately identified source-only/registry QA receipts and diagnostics.
All 75 server digests/sizes and independent unauthenticated public downloads
matched; SHA256SUMS covers the other 74 assets.

| Actual registry archive | SHA256 |
| --- | --- |
| core 1.5.0-rc.2 | 3b6879d14c7f705f865aa63810abfc00edb1eeb6a1b97adc50780e3dac357124 |
| companion 0.1.1-rc.2 | 6d722c64aff691cea61d797965f56c6b49de2926697bb9c16e0618837f975014 |

Core runtime/test/tool/build fingerprint is
`2cb5a923d3b27111ed5ff8c1ca3e76da887cdbc8a83322c8926f7933a41b882a`;
normalized companion fingerprint is
`f9024bf37fb16a02bc532444d880629914d47915c2f17575d6fe724bae064096`.
Both actual Cargo-packaged archives identify clean main
`986169d75e18493d95a36e1134d4a7fe951962ae`, produced by normal PR #10 merge.
The source-only feature archive `4ab2365ec60e2c07569516595af8d63dcd6afda629f4d31e0dcb65bd91a3bf40`
retains its different identity and supplies no substitute registry approval.

- Core: 86400.568117542 seconds, 189106 ACKs, 89 restarts, 17 backups,
  710 publications, 355 each SQ8/prepared-cache/lease checks and 17 generation backups.
- Session/cache: 86400.22028595908 seconds after 100 resident references,
  1449203 requests and 354 sessions. Normal/EOF/kill/idle counts are 89/89/88/88;
  preparations/live-lease refusals/released leases each 354, re-encodings 0,
  invalid-input recoveries 708, before/after/current fixture bytes identical.
- Separate mapped reader: 86400.024580708 seconds, 140577169 records,
  818024 maps and 332 observed generations. It binds the final successful
  publisher receipt SHA256 `d221010b02e4bdc48d278f6909b6bab2971c4689877efa125074f0fbe1c555cb`.

Each actual-registry run completed its own 24-hour clock with completed progress
and exit code 0. The official paired gate accepted all eight core and eight companion
registry receipts and the final publisher/reader pair; the additional session gate
passed independently. Both registry platform checks have no errors. Actual hosts
are Linux, macOS, Windows Server 2025/ProductType 3/AMD64/NTFS and Windows 11 Enterprise/
ProductType 1/ARM64/NTFS, each with Rust 1.89.0 and stable 1.99.1. Tag source CI 41173849574
passed 10 jobs and native CI 41173849735 passed 4 jobs. Fresh actual remote annotated-tag
installations, local registry CLI/library consumers, seven exact docs.rs APIs and
the installed Python wheel's actual-registry CLI consumer also passed.

These gates approve this candidate's frozen implementation for metadata-only
promotion. Stable publication, fresh stable consumers/tagCI and the formal public
release remain separate steps; development timings and external-user observations
keep their original scope.

## RC1 history and scope

Both independent source-archive workloads passed before PR #9 was normally merged
at 8b287675d7738444f5160bf88715f84cd4448242. The mixed run completed
86400.371455375 seconds with 189216 ACKs, 89 restarts, 17 backups and 710 generation
publications. Session/cache completed 86400.4393051248 seconds with 1186375 requests,
100 references, 290 sessions, zero re-encodings and all leases released.

The [published RC1 pair and distribution](https://github.com/gay00ung/vecnook/releases/tag/v1.5.0-rc.1)
passed clean-main/tag CI, actual registry consumers on all required OS/toolchains,
remote tag installs, docs.rs and public asset checks. Its separately started
actual-registry core/mapped/session full24h runs remain preserved. Source-only and
registered-archive identities are different; neither may be silently relabeled.
RC1 evidence cannot approve the changed RC2 runtime or added tool sources. Existing 1.4 registry
packages and receipts remain separate as well.

Before RC2 publication, merge its metadata/documentation PR normally and pass clean
main CI. Then publish and independently verify the exact RC2 core and Cargo-packaged
companion, complete their fresh registry/tag/native consumers and run their own
full24h workloads below. A running or earlier-candidate receipt is not approval.

## Platform checks

The PR runs Linux, macOS, Windows Server x64 and Windows 11 ARM64, each on
Rust 1.89 and current stable, plus directory-sync probes and native ZIP checks.
Source/package tests and actual registry installation are different steps:
registry jobs remain pending until the exact candidate versions are published.
Inspect actual Windows caption/ProductType/architecture/NTFS receipts before
claiming the required desktop/server coverage.

## Independent 24-hour workloads

Before publication, these runners may check one immutable, clean feature-source
archive. Those receipts remain source-only evidence. Formal registry approval uses
the exact downloaded registry archives; a different archive requires fresh full-duration
runs and retains its own identity. Record each archive's SHA256,
normalized source fingerprint, compiled binaries, commands, actual start times
and process IDs. Preserve failed/short runs; never accumulate their durations.

The mixed publisher workload now checks prepared-cache results against resident
SQ8 and exact original reranking, verifies live cache leases, explicitly prunes
after reader release and continues the existing ACK/restart/backup/generation tests.

    python tools/run_soak.py --crate PATH/vecnook-1.5.0-rc.2.crate --output NEW_MIXED_DIR --seconds 86400 --mutations 100000

A separate CLI process workload uses an immutable store plus queries.fvecs,
20 excluded calibration queries and 100 reference queries. It computes resident
references from the same archive, then checks cached sessions' IDs, ordering,
distance bits, payloads, flags and one-time initialization. It repeats malformed/
oversized input recovery, normal close, EOF, deliberate process kill, idle expiry
and cache-lease protection/release. Each run has its own actual 24-hour clock.

    python tools/run_session_soak.py --crate PATH/vecnook-1.5.0-rc.2.crate --fixture IMMUTABLE_FIXTURE --output NEW_SESSION_DIR --seconds 86400 --requests 100000

The default session batch has at most 4,096 queries. After the request floor,
50ms pacing prevents an unnecessary unlimited busy loop. Memory is sampled
separately and is not a native high-water or global system-RAM measurement.
The controlled performance results remain in [development validation](validation-1.5.md).

Both final receipts require successful exit and completed progress. The mixed
run additionally needs elapsed>=86400, ACK>=100000, restarts>=3, backups>=1,
generation publications>=4, SQ8 checks>=2, prepared-cache and cache-lease
checks>=2, and generation backups>=1. Session progress needs elapsed>=86400,
verified requests>=100000, all 100 reference queries, at least four sessions,
all four exit modes, zero session re-encodings and every started cache lease
released. Cache preparations and live-lease refusals must match session starts,
with both invalid-input recoveries checked for every session.
Fingerprints/fixture bytes must remain unchanged.

## Independent paired mapped reader

After the actual RC2 core publisher is running, start the optional companion's
separate reader against that live receipt and immutable generation store:

    python tools/run_acceleration_soak.py --core-crate PATH/vecnook-1.5.0-rc.2.crate --adapter-crate PATH/vecnook-accel-0.1.1-rc.2.crate --publisher-receipt NEW_MIXED_DIR/receipt.json --output NEW_MAPPED_DIR --seconds 86400 --records 100000

It needs its own actual elapsed>=86400, checked records>=100000, mapped checks>=3,
observed generations>=3, completed progress/exit0 and exact paired fingerprints.
Its final receipt must bind the final successful core publisher receipt SHA256.
Keep the additional session/cache full24h gate as well; none of the three clocks
can be combined or supplied by RC1.

## Release approval remains separate

These exact-archive source checks can start before registry publication.
They are not, by themselves, registry distribution approval. A later registry
download must be independently checked against the same archive bytes before
any evidence can be considered for the candidate's release gates. A different
runtime/test/tool/build fingerprint requires a new run; healthy identical runs
must not be restarted just because a heartbeat fires.

Before stable promotion, complete the exact registry/platform consumers, the
existing core/companion paired mapped-reader requirements, source-equivalence,
publication and public-distribution gates in [release readiness](release-readiness.md).
The new session receipt is additional evidence. Local source QA cannot replace
the required mapped companion/registry checks, and excluded P2 receives no approval.
