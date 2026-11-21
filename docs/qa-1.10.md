# 1.10 HTML input candidate QA

The candidate adds an optional Python standard-library static HTML-to-JSONL v1
example with caller-supplied embeddings. Its extraction/whitespace/source-range
contract and bounded inputs are documented in the document example README.
There is no core API, search, storage, embedding-provider or mmap contract change.

The candidate pairs core `1.10.0-rc.1`, Python `1.10.0rc1` and companion
`0.1.8-rc.1`. The companion requires the exact core and retains full-segment mmap.
The packaged example and tests change the frozen source fingerprint. Previous
1.9.1 receipts do not approve this candidate. This document defines required
checks; a pending workload or a passing short run is not release approval.

## Source candidate

- Freeze a clean feature commit, package with Rust 1.89, inspect public package
  contents and fingerprint, then install the exact archive into a fresh location.
- Run `test_html_jsonl.py` from that archive with the installed CLI: text-node
  order, blocks/inline elements, CR/LF and indentation, entities, excluded/nested
  elements, whole-source line ranges, byte boundaries, metadata and vector limits,
  invalid arguments/UTF-8/JSON, output non-overwrite and no output on validation
  failure. Import/export/re-import must preserve the extracted UTF-8, model
  identity, tags, full-u64 ID, maximum positive/negative f32 and signed-zero bits.
- Run all existing document, Rust, package, Python, Node and release-tool checks.
  Normal attached PR CI and native archive CI must pass on the frozen head.
- Run three independent full source workloads: mixed core, native session/cache
  and Python session. Each needs exit 0, its own >=86,400 seconds and >=100,000
  checked mutations/requests plus the exact fixture/hash/lifecycle gates.

## Exact registry candidate and stable promotion

Follow [releasing](releasing.md) and [release readiness](release-readiness.md).
After normal merge and clean-main CI, publish and independently verify the core.
Cargo-package/verify the actual companion against that exact public core and
checksum, pass the actual archive paired gate, then publish the companion.

Run four new, independent registry workloads: core mixed mutations, native
session/cache, Python session and mapped readers. Each needs its own full duration,
count, successful exit, lifecycle and identity evidence. The mapped reader binds
the final successful core publisher receipt. Healthy runs must not be restarted,
and separate runs or wall-clock time cannot be combined.

Check eight actual registry consumers per crate across Linux x64, macOS ARM64,
actual Windows 11 ProductType 1 ARM64 NTFS and Windows Server ProductType 3 x64
NTFS, with Rust 1.89 and stable. Every core distribution runs the packaged HTML
test and existing document/Node suites. Verify fresh-cache CLI/library resolution,
the isolated wheel, docs.rs, remote annotated tag installations and exact tag CI.
Audit native archives, release flags, every public asset's server digest/size,
independent unauthenticated downloads and SHA256SUMS.

Stable promotion needs a separate normal PR/main CI, official unchanged-source
and paired publication gates, core-first publication, actual companion packaging
and final public download audit. A review-only staged companion is never posted.
Historical performance measurements stay under their original versions, and this
example does not supply external-user or real-app adoption evidence.

## Completed RC1 validation

The checked source commit was 6ef95fe0e49a6843a8fae969f60fad819009f4dd. Normal merge preserved its tree at f076fcbfab167fc2f563ec0b0997182b15990a1e. Source-only mixed core, native session/cache and Python session runs each completed their own 24-hour gates before publication. These records remain separate from the public-package runs below.

Exact public core archive SHA-256: cc6b3c9e7b75ae3756e1ee6d896889315eccad722286a0d2a0c1f08a2a7d12ae. Exact companion archive SHA-256: 7da68cfb3d1b20c92887bcc15ac3db552d53e63a7dcb1e6959973589e82bfef6.

| Actual registry workload | Own seconds | Verified count |
| --- | ---: | ---: |
| Core acknowledged mutations | 86400.929404 | 189,186 |
| Native session requests | 86405.234288 | 1,258,723 |
| Python session requests | 86400.067762 | 1,603,296 |
| Mapped record checks | 86400.016313 | 137,400,808 |

All four original runs completed with exit 0 and their own duration/count/lifecycle/identity evidence. The mapped reader observed 349 generations and bound the final completed core publisher receipt.

The remote annotated v1.10.0-rc.1 tag object e81d842b824008de15628ed30dec1c4f8e7f46a3 resolves to f076fcbfab167fc2f563ec0b0997182b15990a1e. Separate fresh-cache tag installations passed all nine HTML tests, the max-f32 JSONL restore and prune regressions, the Node.js app suite and the companion demo.

[Tag CI](https://github.com/gay00ung/vecnook/actions/runs/44872524002) verified eight actual registry CLI/library jobs for each package with Rust 1.89 and stable on Linux x64, macOS ARM64, actual Windows 11 ProductType 1 ARM64 NTFS and Windows Server ProductType 3 x64 NTFS. [Native archive CI](https://github.com/gay00ung/vecnook/actions/runs/44872524055) supplied four core and four companion archives with extracted executable receipts.

The official core and paired release gates passed on these exact archives and completed receipts. Every core job ran 59 document tests, including nine HTML tests, with checkout, packaged and actual registry CLIs. The installed Python 1.10.0rc1 wheel imported/exported extracted HTML through the actual registry CLI, preserving full-u64 IDs, maximum positive/negative finite f32 and signed-zero bits. The [public RC1 release](https://github.com/gay00ung/vecnook/releases/tag/v1.10.0-rc.1) contains 77 verified assets: all server digests and sizes, independent unauthenticated downloads and 76 SHA256SUMS rows passed.

Stable 1.10.0 promotion changes package/dependency versions, lockfiles and release documentation. It still requires its own normal PR/main CI, candidate-equivalence gates, core-first publication, real companion Cargo verification against that public core, registry/tag installations, docs.rs, wheel/native assets and final public-download audit. Candidate completion alone does not approve a stable publication.
