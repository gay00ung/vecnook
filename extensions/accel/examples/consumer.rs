//! External registry consumer; run twice at the same path to verify restart.
use std::path::PathBuf;
use vecnook::{
    Config, EmbeddingSpace, Metric,
    generation::{DistanceKernel, PublishOptions, Publisher, ScalarKernel, Snapshot},
};
use vecnook_accel::{AutoKernel, mapped_snapshot};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("missing application directory")?,
    );
    std::fs::create_dir_all(&root)?;
    let store = root.join("store");
    let mut writer = if store.exists() {
        Publisher::open(&store)?
    } else {
        Publisher::create(
            &store,
            Config::new(5),
            EmbeddingSpace::new("consumer-v1", 5, Metric::SquaredL2),
        )?
    };
    let before = Snapshot::open(&store)?;
    let sequence = before
        .sequence()
        .checked_add(1)
        .ok_or("sequence exhausted")?;
    let vector = [-0.0, 1.0, f32::from_bits(1), -17.5, f32::MAX];
    let metadata = format!("원문-{sequence}");
    let mut builder = writer.begin(sequence, PublishOptions::default())?;
    builder.add(u64::MAX, &vector, &metadata)?;
    builder.add(7, &[0., 2., 3., 4., 5.], "reader pin")?;
    let published = builder.commit()?;
    // SAFETY: this consumer exclusively uses Publisher; no process modifies a
    // published segment. The mapped reader retains its immutable generation pin.
    let mapped = unsafe { mapped_snapshot(published) }?;
    let record = mapped.get(u64::MAX)?.ok_or("missing original")?;
    assert_eq!(record.metadata, metadata);
    assert!(
        record
            .vector
            .iter()
            .zip(vector)
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );
    let kernel = AutoKernel::detect();
    assert_eq!(
        kernel.dot(&vector, &vector).to_bits(),
        ScalarKernel.dot(&vector, &vector).to_bits()
    );
    let exact = mapped.search_exact(&vector, 2)?;
    assert_eq!(
        exact.neighbors,
        mapped
            .quantize()?
            .search_with_kernel(&vector, 2, mapped.len(), &kernel)?
            .neighbors
    );
    let mut next = writer.begin(sequence + 1, PublishOptions::default())?;
    next.add(99, &[1.; 5], "new generation")?;
    drop(next.commit()?);
    writer.collect_garbage()?;
    assert_eq!(mapped.get(u64::MAX)?.unwrap().metadata, metadata);
    let backup = root.join(format!("backup-{sequence}"));
    let restored = mapped.backup(&backup, PublishOptions::default())?;
    assert_eq!(
        restored.get(u64::MAX)?.unwrap().vector[0].to_bits(),
        (-0.0f32).to_bits()
    );
    assert_eq!(Snapshot::open(&store)?.sequence(), sequence + 1);
    drop(before);
    drop(mapped);
    assert!(writer.collect_garbage()? >= 1);
    println!(
        "{{\"schema_version\":1,\"passed\":true,\"sequence\":\"{sequence}\",\"backend\":\"{:?}\",\"checks\":[\"original_bits\",\"sq8_rerank\",\"simd_bits\",\"mapped_pin\",\"generation_refresh\",\"independent_backup\",\"gc\"]}}",
        kernel.acceleration()
    );
    Ok(())
}
