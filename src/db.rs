use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

use crate::{
    BatchReport, Config, Error, IndexStats, Mutation, Record, Result, SearchOptions, SearchReport,
    VectorIndex,
    storage::{self, Operation},
};

#[derive(Clone, Debug, Default)]
/// What opening recovered, trimmed or rebuilt.
#[non_exhaustive]
pub struct RecoveryInfo {
    /// Complete WAL frames applied after the snapshot sequence.
    pub replayed_frames: usize,
    /// Complete WAL frames already represented by the snapshot.
    pub skipped_frames: usize,
    /// Bytes discarded from an incomplete final WAL frame.
    pub truncated_tail_bytes: u64,
    /// Whether a validated checkpoint graph avoided a rebuild.
    pub graph_cache_loaded: bool,
    /// Nodes restored from the graph cache before WAL insertion.
    pub cached_nodes: usize,
    /// Reason a missing/stale/invalid disposable cache was rebuilt.
    pub graph_cache_note: Option<String>,
}

#[derive(Clone, Copy, Debug)]
/// Thresholds for explicit synchronous maintenance. Defaults: 64 MiB WAL,
/// 128 tombstones and 20% tombstones; no background worker is started.
#[non_exhaustive]
pub struct MaintenancePolicy {
    /// Current WAL size, or byte threshold when used in a policy.
    pub wal_bytes: u64,
    /// Minimum deleted-node count before recommending compaction.
    pub min_tombstones: usize,
    /// Deleted/physical ratio, or threshold when used in a policy.
    pub tombstone_ratio: f64,
}
impl Default for MaintenancePolicy {
    fn default() -> Self {
        Self {
            wal_bytes: 64 * 1024 * 1024,
            min_tombstones: 128,
            tombstone_ratio: 0.2,
        }
    }
}
impl MaintenancePolicy {
    /// Set the WAL byte threshold for checkpoint recommendation.
    pub fn with_wal_bytes(mut self, bytes: u64) -> Self {
        self.wal_bytes = bytes;
        self
    }
    /// Set the minimum tombstone count for compaction recommendation.
    pub fn with_min_tombstones(mut self, count: usize) -> Self {
        self.min_tombstones = count;
        self
    }
    /// Set the minimum deleted/physical ratio for compaction recommendation.
    pub fn with_tombstone_ratio(mut self, ratio: f64) -> Self {
        self.tombstone_ratio = ratio;
        self
    }
}

#[derive(Clone, Debug)]
/// Observed storage pressure and maintenance recommendations.
#[non_exhaustive]
pub struct MaintenanceStatus {
    /// Current WAL size, or byte threshold when used in a policy.
    pub wal_bytes: u64,
    /// Deleted/physical ratio, or threshold when used in a policy.
    pub tombstone_ratio: f64,
    /// Whether the WAL meets the checkpoint threshold.
    pub checkpoint_recommended: bool,
    /// Whether both compaction thresholds are met.
    pub compact_recommended: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Action performed by an explicit maintenance call.
#[non_exhaustive]
pub enum MaintenanceAction {
    /// Thresholds did not require maintenance.
    None,
    /// Persisted the current state and cleared the WAL.
    Checkpoint,
    /// Rebuilt active records and reclaimed deleted nodes.
    Compact,
}
#[derive(Clone, Debug)]
/// Result of synchronous maintenance.
#[non_exhaustive]
pub struct MaintenanceReport {
    /// Action actually performed.
    pub action: MaintenanceAction,
    /// Nodes reclaimed by compaction, zero for other actions.
    pub removed_nodes: usize,
}

/// Holds an exclusive OS lock until dropped. Dropping does not checkpoint:
/// completed changes are already synced in the WAL.
pub struct Database {
    path: PathBuf,
    index: VectorIndex,
    wal: File,
    _lock: File,
    sequence: u64,
    poisoned: bool,
    recovery: RecoveryInfo,
}

impl Database {
    /// Create a database and hold its exclusive OS lock.
    /// Creates missing directories; refuses existing database files without overwriting them.
    /// Returns [`Error::Locked`], [`Error::AlreadyExists`], invalid configuration, platform or I/O errors.
    pub fn create(path: impl AsRef<Path>, config: Config) -> Result<Self> {
        let index = VectorIndex::new(config)?;
        Self::create_from_index(path.as_ref(), index)
    }

    pub(crate) fn create_from_index(path: &Path, index: VectorIndex) -> Result<Self> {
        storage::ensure_platform()?;
        let path = path.to_path_buf();
        fs::create_dir_all(&path)?;
        let lock = storage::acquire_lock(&path)?;
        if [
            "snapshot.bin",
            "wal.bin",
            "snapshot.tmp",
            "index.bin",
            "index.tmp",
        ]
        .iter()
        .any(|name| path.join(name).exists())
        {
            return Err(Error::AlreadyExists);
        }
        let wal = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path.join("wal.bin"))?;
        wal.sync_all()?;
        storage::write_snapshot(&path, &index, 0)?;
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            storage::sync_directory(parent)?;
        }
        Ok(Self {
            path,
            index,
            wal,
            _lock: lock,
            sequence: 0,
            poisoned: false,
            recovery: RecoveryInfo::default(),
        })
    }

    /// Open a database, validate its files, recover the WAL and hold an exclusive lock.
    /// Only an incomplete final WAL frame is trimmed. Complete corruption fails with
    /// [`Error::Corrupt`]; a missing WAL is an I/O error. Even readers need the exclusive lock.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        storage::ensure_platform()?;
        let path = path.as_ref().to_path_buf();
        let lock = storage::acquire_lock(&path)?;
        // Missing WAL is not silently recreated: acknowledged writes could
        // have been removed independently of a valid older snapshot.
        let mut wal = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path.join("wal.bin"))?;
        let recovered = storage::recover(&path, &mut wal)?;
        let cached_nodes = recovered.graph.as_ref().map_or(0, |g| g.links.len());
        let graph_cache_loaded = recovered.graph.is_some();
        let index = match recovered.graph {
            Some(graph) => {
                VectorIndex::from_cached_graph(recovered.config, recovered.records, graph)?
            }
            None => VectorIndex::from_records(recovered.config, recovered.records)?,
        };
        let recovery = RecoveryInfo {
            replayed_frames: recovered.replayed,
            skipped_frames: recovered.skipped,
            truncated_tail_bytes: recovered.truncated_bytes,
            graph_cache_loaded,
            cached_nodes,
            graph_cache_note: recovered.cache_note,
        };
        Ok(Self {
            path,
            index,
            wal,
            _lock: lock,
            sequence: recovered.sequence,
            poisoned: false,
            recovery,
        })
    }

    /// Persisted dimensions, metric and graph construction parameters.
    pub fn config(&self) -> &Config {
        self.index.config()
    }
    /// Current record, graph and coordinate-payload counts.
    pub fn stats(&self) -> IndexStats {
        self.index.stats()
    }
    /// Bounded snapshot/node headroom; this does not measure process RSS.
    pub fn capacity(&self) -> crate::CapacityStatus {
        crate::CapacityStatus::from_records(self.index.records())
    }
    /// Export active IDs, original f32 coordinates and opaque metadata to a new file.
    /// Failure can leave an incomplete export; import verifies it before writing.
    pub fn export(&self, destination: impl AsRef<Path>) -> Result<()> {
        self.ensure_writable()?;
        crate::transfer::write(destination.as_ref(), &self.index, None)
    }
    /// Import a validated logical vector export into a new directory.
    /// Existing destinations are never reused. A storage failure can leave a directory
    /// requiring inspection/removal before retrying; no partially populated snapshot is written.
    pub fn import(source: impl AsRef<Path>, destination: impl AsRef<Path>) -> Result<Self> {
        let data = crate::transfer::read(source.as_ref())?;
        if data.space.is_some() {
            return Err(Error::InvalidInput(
                "use Collection::import for a document export".into(),
            ));
        }
        let index = VectorIndex::from_records(data.config, data.records)?;
        crate::transfer::claim_directory(destination.as_ref())?;
        Self::create_from_index(destination.as_ref(), index)
    }
    pub(crate) fn export_collection(
        &self,
        destination: &Path,
        space: &crate::EmbeddingSpace,
    ) -> Result<()> {
        self.ensure_writable()?;
        crate::transfer::write(destination, &self.index, Some(space))
    }
    /// Number of active IDs, without scanning graph edges.
    pub fn len(&self) -> usize {
        self.index.len()
    }
    /// Whether no records are active.
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }
    /// Last committed WAL frame sequence; one sequence per nonempty batch.
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    /// Recovery and graph cache observations from the most recent open.
    pub fn recovery_info(&self) -> &RecoveryInfo {
        &self.recovery
    }
    /// Find an active ID. Deleted and replaced versions are not returned.
    pub fn get(&self, id: u64) -> Option<&Record> {
        self.index.get(id)
    }
    /// Iterate active original records in ascending ID order while borrowing this handle.
    pub fn iter(&self) -> impl Iterator<Item = &Record> {
        self.index.iter()
    }
    /// Exhaustively search active vectors, ordered by distance then ID.
    /// K is capped to active count; K=0 returns no neighbors. Invalid queries return [`Error::InvalidInput`].
    pub fn search_exact(&self, query: &[f32], k: usize) -> Result<SearchReport> {
        self.index.search_exact(query, k)
    }
    /// Force approximate graph search with an efSearch candidate pool.
    /// Requires efSearch in 1..4096 and at least min(K, active count).
    /// `complete` reports candidate count; measure recall separately.
    pub fn search_hnsw(&self, query: &[f32], k: usize, ef: usize) -> Result<SearchReport> {
        self.index.search_hnsw(query, k, ef)
    }
    /// Search with the requested strategy and report Auto fallback reasons.
    /// Invalid dimensions, nonfinite coordinates or search options return [`Error::InvalidInput`].
    pub fn search(&self, query: &[f32], k: usize, options: SearchOptions) -> Result<SearchReport> {
        self.index.search(query, k, options)
    }
    /// Search exact metadata equality through the maintained inverted index.
    /// Hash bucket candidates are checked against the full string.
    pub fn search_metadata(
        &self,
        query: &[f32],
        k: usize,
        options: SearchOptions,
        metadata: &str,
    ) -> Result<SearchReport> {
        self.index.search_metadata(query, k, options, metadata)
    }
    /// Search only selected IDs; absent and duplicate IDs are ignored.
    /// Rejects more than 100,000 input IDs. Other records are not scanned for eligibility.
    pub fn search_ids(
        &self,
        query: &[f32],
        k: usize,
        options: SearchOptions,
        ids: &[u64],
    ) -> Result<SearchReport> {
        self.index.search_ids(query, k, options, ids)
    }
    /// Evaluate a predicate once per active record before selecting Top-K.
    /// Ineligible nodes can serve as graph paths. Predicate evaluation costs O(active count).
    /// Auto repairs an underfilled result exactly; complete approximate results can still have low recall.
    pub fn search_filtered<F>(
        &self,
        query: &[f32],
        k: usize,
        options: SearchOptions,
        filter: F,
    ) -> Result<SearchReport>
    where
        F: FnMut(&Record) -> bool,
    {
        self.index.search_filtered(query, k, options, filter)
    }
    /// Validate active IDs, vector values, graph bounds and layer structure.
    /// Reports [`Error::Corrupt`] on an inconsistent in-memory index.
    pub fn check_invariants(&self) -> Result<()> {
        self.index.check_invariants()
    }

    /// Insert or replace an ID after syncing its WAL frame; true means a new active ID.
    /// Input errors leave storage unchanged. An I/O error poisons writes and may have persisted:
    /// close, reopen and inspect the ID. Metadata is bounded to 16 KiB of UTF-8.
    pub fn put(&mut self, id: u64, vector: &[f32], metadata: &str) -> Result<bool> {
        self.ensure_writable()?;
        self.index.validate_put(vector, metadata)?;
        let next = self.next_sequence()?;
        let frame = storage::encode_frame(
            next,
            Operation::Put {
                id,
                vector,
                metadata,
            },
        );
        self.commit_frame(&frame)?;
        let result = self.index.put(id, vector, metadata);
        match result {
            Ok(inserted) => {
                self.sequence = next;
                Ok(inserted)
            }
            Err(error) => {
                self.poisoned = true;
                Err(error)
            }
        }
    }

    /// Durably delete an active ID; false means it was already absent and no frame was written.
    /// An I/O failure may have persisted and poisons further writes.
    pub fn delete(&mut self, id: u64) -> Result<bool> {
        self.ensure_writable()?;
        if self.index.get(id).is_none() {
            return Ok(false);
        }
        let next = self.next_sequence()?;
        self.commit_frame(&storage::encode_frame(next, Operation::Delete { id }))?;
        self.index.delete(id);
        self.sequence = next;
        Ok(true)
    }

    /// One synced WAL frame. Input errors or WAL write failure leave RAM unchanged.
    pub fn write_batch(&mut self, operations: &[Mutation<'_>]) -> Result<BatchReport> {
        self.ensure_writable()?;
        let mut report = self.index.validate_batch(operations)?;
        report.sequence = self.sequence;
        if report.inserted + report.updated + report.deleted == 0 {
            return Ok(report);
        }
        let next = self.next_sequence()?;
        self.commit_frame(&storage::encode_batch(next, operations))?;
        for operation in operations {
            match operation {
                Mutation::Put {
                    id,
                    vector,
                    metadata,
                } => {
                    if let Err(error) = self.index.put(*id, vector, metadata) {
                        self.poisoned = true;
                        return Err(error);
                    }
                }
                Mutation::Delete { id } => {
                    self.index.delete(*id);
                }
            }
        }
        self.sequence = next;
        report.sequence = next;
        Ok(report)
    }

    /// Make an independently openable backup in a new directory. A failed copy
    /// can leave an incomplete destination; it never overwrites an existing one.
    pub fn backup(&mut self, destination: impl AsRef<Path>) -> Result<()> {
        self.ensure_writable()?;
        let destination = destination.as_ref();
        if destination.exists() {
            return Err(Error::AlreadyExists);
        }
        if let Some(parent) = destination.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        fs::create_dir(destination)?;
        let _target_lock = storage::acquire_lock(destination)?;
        self.checkpoint()?;
        for name in ["snapshot.bin", "index.bin"] {
            let mut source = File::open(self.path.join(name))?;
            let mut target = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(destination.join(name))?;
            std::io::copy(&mut source, &mut target)?;
            target.sync_all()?;
        }
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(destination.join("wal.bin"))?
            .sync_all()?;
        storage::sync_directory(destination)?;
        if let Some(parent) = destination.parent().filter(|p| !p.as_os_str().is_empty()) {
            storage::sync_directory(parent)?;
        }
        Ok(())
    }

    /// Inspect WAL size and tombstones without changing files.
    /// Rejects zero byte/count thresholds or a ratio outside 0..=1.
    pub fn maintenance_status(&self, policy: MaintenancePolicy) -> Result<MaintenanceStatus> {
        if policy.wal_bytes == 0
            || policy.min_tombstones == 0
            || !(0.0..=1.0).contains(&policy.tombstone_ratio)
        {
            return Err(Error::InvalidInput("invalid maintenance thresholds".into()));
        }
        let stats = self.stats();
        let ratio = if stats.physical_nodes == 0 {
            0.0
        } else {
            stats.tombstones as f64 / stats.physical_nodes as f64
        };
        let wal_bytes = self.wal.metadata()?.len();
        Ok(MaintenanceStatus {
            wal_bytes,
            tombstone_ratio: ratio,
            checkpoint_recommended: wal_bytes >= policy.wal_bytes,
            compact_recommended: stats.tombstones >= policy.min_tombstones
                && ratio >= policy.tombstone_ratio,
        })
    }

    /// Synchronously compact if recommended, otherwise checkpoint if recommended.
    /// Does no background work. I/O errors poison further writes.
    pub fn maintain(&mut self, policy: MaintenancePolicy) -> Result<MaintenanceReport> {
        self.ensure_writable()?;
        let status = self.maintenance_status(policy)?;
        let (action, removed_nodes) = if status.compact_recommended {
            (MaintenanceAction::Compact, self.compact()?)
        } else if status.checkpoint_recommended {
            self.checkpoint()?;
            (MaintenanceAction::Checkpoint, 0)
        } else {
            (MaintenanceAction::None, 0)
        };
        Ok(MaintenanceReport {
            action,
            removed_nodes,
        })
    }

    /// Sync a snapshot and graph cache, then truncate and sync the WAL.
    /// I/O failure poisons writes; reopen to determine the persisted state.
    pub fn checkpoint(&mut self) -> Result<()> {
        self.ensure_writable()?;
        let result = storage::write_snapshot(&self.path, &self.index, self.sequence)
            .and_then(|()| storage::clear_wal(&mut self.wal));
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }

    /// Rebuild active records, persist them and reclaim tombstones; returns removed node count.
    /// Temporarily holds a second index. I/O failure poisons writes.
    pub fn compact(&mut self) -> Result<usize> {
        self.ensure_writable()?;
        let replacement = self.index.compacted()?;
        let removed = self.index.stats().physical_nodes - replacement.stats().physical_nodes;
        let result = storage::write_snapshot(&self.path, &replacement, self.sequence)
            .and_then(|()| storage::clear_wal(&mut self.wal));
        if let Err(error) = result {
            self.poisoned = true;
            return Err(error);
        }
        self.index = replacement;
        Ok(removed)
    }

    fn ensure_writable(&self) -> Result<()> {
        if self.poisoned {
            Err(Error::Poisoned)
        } else {
            Ok(())
        }
    }
    fn next_sequence(&self) -> Result<u64> {
        self.sequence
            .checked_add(1)
            .ok_or_else(|| Error::InvalidInput("change sequence exhausted".into()))
    }
    fn commit_frame(&mut self, frame: &[u8]) -> Result<()> {
        let result = storage::append_frame(&mut self.wal, frame);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_batch_wal_write_applies_none_of_the_members_and_poisons_handle() {
        let path =
            std::env::temp_dir().join(format!("vecnook-batch-io-failure-{}", std::process::id()));
        let mut db = Database::create(&path, Config::new(1)).unwrap();
        db.put(1, &[1.0], "original").unwrap();
        db.wal = File::open(path.join("wal.bin")).unwrap();
        assert!(matches!(
            db.write_batch(&[
                Mutation::Delete { id: 1 },
                Mutation::Put {
                    id: 2,
                    vector: &[2.0],
                    metadata: "new"
                }
            ]),
            Err(Error::Io(_))
        ));
        assert_eq!(db.get(1).unwrap().metadata, "original");
        assert!(db.get(2).is_none());
        assert_eq!(db.sequence(), 1);
        assert!(matches!(db.write_batch(&[]), Err(Error::Poisoned)));
        drop(db);
        let db = Database::open(&path).unwrap();
        assert!(db.get(1).is_some());
        assert!(db.get(2).is_none());
        drop(db);
        fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn failed_wal_write_keeps_memory_and_poison_blocks_subsequent_writes() {
        let path = std::env::temp_dir().join(format!("vector-io-failure-{}", std::process::id()));
        let mut db = Database::create(&path, Config::new(2)).unwrap();
        db.put(1, &[1.0, 2.0], "original").unwrap();
        db.wal = File::open(path.join("wal.bin")).unwrap();
        assert!(matches!(
            db.put(1, &[3.0, 4.0], "replacement"),
            Err(Error::Io(_))
        ));
        assert_eq!(db.get(1).unwrap().metadata, "original");
        assert!(matches!(db.put(2, &[0.0, 0.0], ""), Err(Error::Poisoned)));
        assert!(matches!(db.delete(1), Err(Error::Poisoned)));
        assert!(matches!(db.checkpoint(), Err(Error::Poisoned)));
        drop(db);
        let db = Database::open(&path).unwrap();
        assert_eq!(db.get(1).unwrap().vector, [1.0, 2.0]);
        drop(db);
        fs::remove_dir_all(path).unwrap();
    }
}
