use crate::{
    Config, EmbeddingSpace, Error, Record, Result,
    config::{MAX_RECORDS, MAX_SNAPSHOT_BYTES, SNAPSHOT_HEADER_BYTES},
    storage,
};
use std::{
    fs::{self, File},
    path::Path,
};

/// Snapshot/node budget, including superseded records. Counts are not process RSS.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct CapacityStatus {
    /// Visible IDs.
    pub active_records: usize,
    /// All allocated nodes, including deleted versions.
    pub physical_nodes: usize,
    /// Deleted/superseded nodes reclaimed by compaction.
    pub tombstones: usize,
    /// Raw f32 coordinate bytes, excluding text, edges and allocator overhead.
    pub vector_bytes: usize,
    /// Estimated encoded snapshot size if checkpointed now, including header/CRC.
    pub snapshot_bytes: usize,
    /// Snapshot cap in bytes.
    pub max_snapshot_bytes: usize,
    /// Remaining encoded snapshot budget.
    pub remaining_snapshot_bytes: usize,
    /// Physical node cap.
    pub max_physical_nodes: usize,
    /// Additional physical nodes that fit the node cap; byte budget also applies.
    pub remaining_nodes: usize,
}
impl CapacityStatus {
    pub(crate) fn from_records<'a>(records: impl Iterator<Item = &'a Record>) -> Self {
        let mut physical_nodes = 0;
        let mut active_records = 0;
        let mut vector_bytes = 0;
        let mut snapshot_bytes = SNAPSHOT_HEADER_BYTES + 4;
        for record in records {
            physical_nodes += 1;
            active_records += usize::from(!record.deleted);
            vector_bytes += 4 * record.vector.len();
            snapshot_bytes += record.encoded_len();
        }
        Self {
            active_records,
            physical_nodes,
            tombstones: physical_nodes - active_records,
            vector_bytes,
            snapshot_bytes,
            max_snapshot_bytes: MAX_SNAPSHOT_BYTES,
            remaining_snapshot_bytes: MAX_SNAPSHOT_BYTES.saturating_sub(snapshot_bytes),
            max_physical_nodes: MAX_RECORDS,
            remaining_nodes: MAX_RECORDS.saturating_sub(physical_nodes),
        }
    }
}

/// Read-only validation of authoritative records and disposable graph cache.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct DiagnosticReport {
    /// Validated stored index settings.
    pub config: Config,
    /// Embedding identity when inspecting a document collection.
    pub space: Option<EmbeddingSpace>,
    /// Last complete committed sequence.
    pub sequence: u64,
    /// Current record budget after interpreting complete WAL frames in memory.
    pub capacity: CapacityStatus,
    /// Current snapshot file size.
    pub snapshot_file_bytes: u64,
    /// Current WAL file size.
    pub wal_bytes: u64,
    /// Current cache size, or zero when absent.
    pub cache_bytes: u64,
    /// Incomplete tail bytes observed and left untouched.
    pub pending_tail_bytes: u64,
    /// Complete frames newer than the snapshot.
    pub replayed_frames: usize,
    /// Whether the existing cache validated.
    pub graph_cache_valid: bool,
    /// Why an invalid/missing derived cache should be rebuilt by normal open.
    pub graph_cache_note: Option<String>,
    /// Default-policy compaction recommendation.
    pub compact_recommended: bool,
    /// Default-policy WAL checkpoint recommendation.
    pub checkpoint_recommended: bool,
}

/// Inspect under an exclusive lock opened read-only. Never create files, trim WAL,
/// checkpoint or rebuild a cache. Concurrent open returns Locked; complete corruption
/// fails closed. An incomplete final frame is reported and ignored in the report.
pub fn doctor(path: impl AsRef<Path>) -> Result<DiagnosticReport> {
    storage::ensure_platform()?;
    let path = path.as_ref();
    let lock = File::open(path.join("LOCK"))?;
    match lock.try_lock() {
        Ok(()) => (),
        Err(std::fs::TryLockError::WouldBlock) => return Err(Error::Locked),
        Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
    }
    let mut wal = File::open(path.join("wal.bin"))?;
    let recovered = storage::inspect(path, &mut wal)?;
    let space = if path.join("collection.bin").exists() {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| Error::InvalidInput("collection directory needs a UTF-8 name".into()))?;
        let space = crate::collection::read_header(path, name)?;
        if space.dimensions != recovered.config.dimensions
            || space.metric != recovered.config.metric
        {
            return Err(Error::Corrupt("collection header/config mismatch".into()));
        }
        for record in recovered.records.iter().filter(|r| !r.deleted) {
            if crate::Document::from_payload(&record.metadata)?.id != record.id {
                return Err(Error::Corrupt("document ID mismatch".into()));
            }
        }
        Some(space)
    } else {
        None
    };
    let capacity = CapacityStatus::from_records(recovered.records.iter());
    let wal_bytes = wal.metadata()?.len();
    let cache_bytes = match fs::metadata(path.join("index.bin")) {
        Ok(m) => m.len(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
        Err(e) => return Err(e.into()),
    };
    let policy = crate::MaintenancePolicy::default();
    let ratio = capacity.tombstones as f64 / capacity.physical_nodes.max(1) as f64;
    Ok(DiagnosticReport {
        config: recovered.config,
        space,
        sequence: recovered.sequence,
        compact_recommended: capacity.tombstones >= policy.min_tombstones
            && ratio >= policy.tombstone_ratio,
        checkpoint_recommended: wal_bytes >= policy.wal_bytes,
        capacity,
        snapshot_file_bytes: fs::metadata(path.join("snapshot.bin"))?.len(),
        wal_bytes,
        cache_bytes,
        pending_tail_bytes: recovered.truncated_bytes,
        replayed_frames: recovered.replayed,
        graph_cache_valid: recovered.graph.is_some(),
        graph_cache_note: recovered.cache_note,
    })
}
