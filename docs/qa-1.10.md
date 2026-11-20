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
