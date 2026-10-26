# vecnook-accel

Optional runtime-detected AVX2/NEON distance kernels and immutable-generation
memory mapping for Vecnook. The portable `vecnook` core remains dependency-free
and forbids unsafe code. This companion isolates its platform intrinsics and OS
mapping calls, supports Rust 1.89, and depends on one exact core release.

This source is the `0.1.0-rc.1` candidate paired with `vecnook 1.4.0-rc.1`.
Stable promotion requires both packages' registry consumers and fresh full-duration
QA. Check the repository's `docs/validation-1.4.md` for the actual release status.

After the candidate is registered:

```bash
cargo install vecnook-accel --version '=0.1.0-rc.1' --locked
vecnook-accel demo data/new-accel-demo
vecnook-accel search data/new-accel-demo 0,0,1 1 --sq8 2 --mmap
```

The demo requires a new directory and creates an independent `.backup` directory.
It checks original signed-zero/full-u64 values, old-reader pinning across a new
publication, original-f32 SQ8 reranking and backup. Search uses JSON v1 with IDs
and generation/sequence values encoded as decimal strings.

```toml
[dependencies]
vecnook = "=1.4.0-rc.1"
vecnook-accel = "=0.1.0-rc.1"
```

```rust,no_run
use vecnook::generation::{Snapshot, ScalarKernel};
use vecnook_accel::{AutoKernel, mapped_snapshot};

# fn main() -> vecnook::Result<()> {
let reader = Snapshot::open("data/generations")?;
// SAFETY: the application uses Publisher for every store change and prevents
// other programs from editing/truncating published segment files. Publisher
// creates new immutable files and honors reader pins until this view is dropped.
let mapped = unsafe { mapped_snapshot(reader) }?;
let kernel = AutoKernel::detect();
let query = vec![1.0; mapped.space().dimensions];
let reference = mapped.search_filtered(&query, 10, &ScalarKernel, |_, _| true)?;
let accelerated = mapped.search_filtered(&query, 10, &kernel, |_, _| true)?;
assert_eq!(reference.neighbors, accelerated.neighbors);
# Ok(())
# }
```

`mapped_snapshot` is unsafe: every mapped segment, including its file length,
must remain immutable until the returned reader is dropped. Read-only mappings
and cooperative reader pins cannot stop unrelated Unix processes from modifying
a file. Use positional-I/O `Snapshot` without this constructor when the application
cannot uphold that condition. Mapping checksums detect prior accidental damage;
they do not authenticate files or permit later mutation.

Mappings retain file handles and shared OS generation leases through unmapping.
Readers can remain on an old generation while a publisher creates a new one.
Garbage collection skips pinned generations; the OS releases a killed process's
lease. Do not directly remove or rename generation files.

SIMD is an explicit arithmetic opt-in. `AutoKernel::detect()` reports the actual
backend and falls back to scalar on unsupported CPUs. It preserves left-to-right
f64 additions of original f32 products/differences; no horizontal reassociation
or FMA changes rankings. This constraint can limit speedups. mmap can increase
full RSS as original pages become resident. SQ8 retains originals and adds derived
codes/parameters. Measure recall, code build/open time, latency and full RSS for
your workload; none of these options guarantees lower latency or memory.

`examples/consumer.rs` exercises a registry library consumer and restart.
`examples/engine_bench.rs` compares readers against an independent exact oracle;
`examples/accel_soak.rs` is a separate process reader for the paired mutation soak.
The core repository contains the checksum-bound distribution and release gates.
