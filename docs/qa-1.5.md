# 1.5 source candidate QA

The source candidate is core 1.5.0-rc.1, Python 1.5.0rc1 and companion
0.1.1-rc.1 with an exact core dependency. P2 bounded-window mmap is excluded.
This PR and its source QA do not publish registry versions, merge main or announce
a stable release. Existing 1.4 registry packages and receipts remain separate.

## Platform checks

The PR runs Linux, macOS, Windows Server x64 and Windows 11 ARM64, each on
Rust 1.89 and current stable, plus directory-sync probes and native ZIP checks.
Source/package tests and actual registry installation are different steps:
registry jobs remain pending until the exact candidate versions are published.
Inspect actual Windows caption/ProductType/architecture/NTFS receipts before
claiming the required desktop/server coverage.

## Independent 24-hour workloads

Use one immutable, clean-commit core archive for both runs. Record its SHA256,
normalized source fingerprint, compiled binaries, commands, actual start times
and process IDs. Preserve failed/short runs; never accumulate their durations.

The mixed publisher workload now checks prepared-cache results against resident
SQ8 and exact original reranking, verifies live cache leases, explicitly prunes
after reader release and continues the existing ACK/restart/backup/generation tests.

    python tools/run_soak.py --crate PATH/vecnook-1.5.0-rc.1.crate --output NEW_MIXED_DIR --seconds 86400 --mutations 100000

A separate CLI process workload uses an immutable store plus queries.fvecs,
20 excluded calibration queries and 100 reference queries. It computes resident
references from the same archive, then checks cached sessions' IDs, ordering,
distance bits, payloads, flags and one-time initialization. It repeats malformed/
oversized input recovery, normal close, EOF, deliberate process kill, idle expiry
and cache-lease protection/release. Each run has its own actual 24-hour clock.

    python tools/run_session_soak.py --crate PATH/vecnook-1.5.0-rc.1.crate --fixture IMMUTABLE_FIXTURE --output NEW_SESSION_DIR --seconds 86400 --requests 100000

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
