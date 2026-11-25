# 1.11.1 candidate QA

The 1.11.0 CLI writes a JSONL export for 10,001 active documents. The
JSONL importer accepts at most 10,000 and refuses that export. Native binary
transfer restores all 10,001 documents. This patch applies the existing
count limit before creating a JSONL file. The native transfer remains
available for larger collections.

The candidate pairs core `1.11.1-rc.1`, Python wheel `1.11.1rc1` and
companion `0.1.10-rc.1`. The companion pins the exact core and uses
full-segment mmap.

## Source candidate

- Freeze a clean feature commit and package with Rust 1.89. Check the
  packaged contents and independent source fingerprints.
- Run the actual CLI regression on checkout and packaged distributions.
  Compare all records after restoring exactly 10,000 documents. At 10,001,
  require a nonzero export status, absent output and intact source records.
  Keep the existing f32, prune, HTML, SQLite and Node app checks.
- Run fresh mixed core, native cache/session and Python session workloads.
  Each needs its own duration of at least 86,400 seconds, exit 0, 100,000
  verified operations, exact artifact hashes and lifecycle evidence.

## Registry candidate and stable promotion

Follow [releasing](releasing.md) and [release readiness](release-readiness.md).
After normal PR CI, a normal merge and clean-main CI, publish the core and
verify the public checksum by independent download. Package and verify the
actual companion against that public core. Pass the actual archive paired
gate before publishing the companion. Staged review adapters serve package
review only.

Run four fresh actual registry 24-hour workloads. The mapped reader binds
the successful completed core publisher receipt. Verify the exact core
and companion CLI and library consumers on Linux x64, macOS ARM64,
Windows 11 ProductType 1 ARM64 NTFS and Windows Server ProductType 3 x64
NTFS, with Rust 1.89 and stable. Check the isolated wheel against the
installed registry CLI, docs.rs exact versions, fresh annotated remote tag
installs, tag CI, real native executables and every public asset digest,
size and SHA256SUMS row by independent unauthenticated download.

Stable promotion requires its own PR and clean-main CI, official
candidate equivalence, core-first paired publication, installations,
wheel and native checks and final public download verification.
