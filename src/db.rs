use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

use crate::{
    Config, Error, IndexStats, Record, Result, SearchReport, VectorIndex,
    storage::{self, Operation},
};

#[derive(Clone, Debug, Default)]
pub struct RecoveryInfo {
    pub replayed_operations: usize,
    pub skipped_operations: usize,
    pub truncated_tail_bytes: u64,
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
    pub fn create(path: impl AsRef<Path>, config: Config) -> Result<Self> {
        storage::ensure_platform()?;
        let index = VectorIndex::new(config)?;
        let path = path.as_ref().to_path_buf();
        fs::create_dir_all(&path)?;
        let lock = storage::acquire_lock(&path)?;
        if ["snapshot.bin", "wal.bin", "snapshot.tmp"]
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
        let index = VectorIndex::from_records(recovered.config, recovered.records)?;
        let recovery = RecoveryInfo {
            replayed_operations: recovered.replayed,
            skipped_operations: recovered.skipped,
            truncated_tail_bytes: recovered.truncated_bytes,
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

    pub fn config(&self) -> &Config {
        self.index.config()
    }
    pub fn stats(&self) -> IndexStats {
        self.index.stats()
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn recovery_info(&self) -> &RecoveryInfo {
        &self.recovery
    }
    pub fn get(&self, id: u64) -> Option<&Record> {
        self.index.get(id)
    }
    pub fn search_exact(&self, query: &[f32], k: usize) -> Result<SearchReport> {
        self.index.search_exact(query, k)
    }
    pub fn search_hnsw(&self, query: &[f32], k: usize, ef: usize) -> Result<SearchReport> {
        self.index.search_hnsw(query, k, ef)
    }
    pub fn check_invariants(&self) -> Result<()> {
        self.index.check_invariants()
    }

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

    pub fn checkpoint(&mut self) -> Result<()> {
        self.ensure_writable()?;
        let result = storage::write_snapshot(&self.path, &self.index, self.sequence)
            .and_then(|()| storage::clear_wal(&mut self.wal));
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }

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
