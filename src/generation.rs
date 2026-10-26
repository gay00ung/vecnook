//! Segmented immutable generations, pinned process readers and optional SQ8 candidates.
//!
//! Publication writes original f32 records into bounded, checksummed segments and
//! atomically replaces a small CURRENT pointer only after the complete generation
//! is synced. Existing readers retain their generation. This is a separate bulk
//! snapshot store; it does not change Database's WAL, locking or capacity contracts.
use crate::{Config, EmbeddingSpace, Error, Metric, Neighbor, Record, Result, storage, stream};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BinaryHeap},
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

mod format;
mod quantization;
#[cfg(test)]
mod tests;
pub use quantization::QuantizedSnapshot;

const MAX_RECORDS: usize = 1_000_000;
const MAX_SEGMENTS: usize = 1024;
const MAX_SEGMENT_BYTES: usize = 64 * 1024 * 1024;
const MAX_MANIFEST_BYTES: usize = 64 * 1024 * 1024;
const MAX_RESULTS: usize = 10_000;

/// Upper bounds for one publication. These do not change the legacy database.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct PublishOptions {
    /// Maximum encoded bytes per segment, including its checksum; 64 KiB..=64 MiB.
    pub segment_bytes: usize,
    /// Maximum records in the new generation; 1..=1,000,000.
    pub max_records: usize,
}
impl Default for PublishOptions {
    fn default() -> Self {
        Self {
            segment_bytes: 16 * 1024 * 1024,
            max_records: MAX_RECORDS,
        }
    }
}
impl PublishOptions {
    /// Choose the bounded segment size.
    pub fn with_segment_bytes(mut self, bytes: usize) -> Self {
        self.segment_bytes = bytes;
        self
    }
    /// Set a smaller application record bound.
    pub fn with_max_records(mut self, count: usize) -> Self {
        self.max_records = count;
        self
    }
    fn validate(self) -> Result<()> {
        if !(64 * 1024..=MAX_SEGMENT_BYTES).contains(&self.segment_bytes)
            || !(1..=MAX_RECORDS).contains(&self.max_records)
        {
            return Err(Error::InvalidInput(
                "generation publication bounds are invalid".into(),
            ));
        }
        Ok(())
    }
}

/// A validated, original record returned by a segmented reader.
/// Coordinates and metadata retain their original bits/UTF-8 bytes.
pub type SnapshotRecord = Record;

/// Distance arithmetic for exact scans and original-vector reranking.
///
/// Implementations receive equal-length, finite coordinates. They must return
/// the same ordered f64 arithmetic as ScalarKernel, without fused reassociation.
/// A separate optional platform crate can implement acceleration.
pub trait DistanceKernel: Send + Sync {
    /// Dot product, adding individual products from left to right.
    fn dot(&self, left: &[f32], right: &[f32]) -> f64;
    /// Squared L2, adding squared f64 differences from left to right.
    fn squared_l2(&self, left: &[f32], right: &[f32]) -> f64;
}

/// Portable standard-library reference arithmetic.
pub struct ScalarKernel;
impl DistanceKernel for ScalarKernel {
    fn dot(&self, left: &[f32], right: &[f32]) -> f64 {
        left.iter()
            .zip(right)
            .map(|(&a, &b)| f64::from(a) * f64::from(b))
            .sum()
    }
    fn squared_l2(&self, left: &[f32], right: &[f32]) -> f64 {
        crate::math::squared_l2(left, right)
    }
}

/// Positional read source for one immutable encoded segment.
///
/// Sources must keep returning the same bytes. The reader validates replacement
/// sources before installing them. The safe default uses positional file I/O.
pub trait SegmentReader: Send + Sync {
    /// Encoded byte count, including the trailing checksum.
    fn len(&self) -> u64;
    /// Whether the source is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Fill the complete destination, or return an I/O error.
    fn read_exact_at(&self, offset: u64, destination: &mut [u8]) -> io::Result<()>;
}

struct Pin {
    _lease: File,
}

/// A read-only segment handle retaining its generation's process-reader pin.
#[derive(Clone)]
pub struct SegmentFile {
    file: Arc<File>,
    length: u64,
    _pin: Arc<Pin>,
}
impl SegmentFile {
    /// Borrow the read-only file for an optional platform mapping implementation.
    /// Keep this SegmentFile alive for the entire lifetime of the mapping.
    pub fn file(&self) -> &File {
        &self.file
    }
}
impl SegmentReader for SegmentFile {
    fn len(&self) -> u64 {
        self.length
    }
    fn read_exact_at(&self, mut offset: u64, mut destination: &mut [u8]) -> io::Result<()> {
        if offset
            .checked_add(destination.len() as u64)
            .is_none_or(|end| end > self.length)
        {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        while !destination.is_empty() {
            #[cfg(unix)]
            let read = {
                use std::os::unix::fs::FileExt;
                self.file.read_at(destination, offset)
            };
            #[cfg(windows)]
            let read = {
                use std::os::windows::fs::FileExt;
                self.file.seek_read(destination, offset)
            };
            #[cfg(not(any(unix, windows)))]
            let read: io::Result<usize> = Err(io::ErrorKind::Unsupported.into());
            match read {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(count) => {
                    offset += count as u64;
                    destination = &mut destination[count..];
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct Entry {
    id: u64,
    segment: u32,
    offset: u32,
    metadata_len: u32,
}
#[derive(Clone, Copy)]
struct SegmentInfo {
    length: u64,
    crc: u32,
}
struct Manifest {
    generation: u64,
    sequence: u64,
    config: Config,
    space: EmbeddingSpace,
    segments: Vec<SegmentInfo>,
    entries: Vec<Entry>,
}

/// One exclusive bulk publisher; process readers do not hold this writer lock.
pub struct Publisher {
    path: PathBuf,
    config: Config,
    space: EmbeddingSpace,
    _writer: File,
    poisoned: bool,
}
impl Publisher {
    /// Create a separate, new generation store and publish its empty generation.
    /// Existing paths are refused. No legacy database files are modified.
    pub fn create(path: impl AsRef<Path>, config: Config, space: EmbeddingSpace) -> Result<Self> {
        storage::ensure_platform()?;
        config.validate()?;
        space.validate()?;
        if config.dimensions != space.dimensions || config.metric != space.metric {
            return Err(Error::EmbeddingMismatch);
        }
        let path = path.as_ref().to_path_buf();
        match fs::create_dir(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(Error::AlreadyExists);
            }
            Err(error) => return Err(error.into()),
        }
        let path = fs::canonicalize(path)?;
        let writer = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path.join("WRITER"))?;
        writer.try_lock().map_err(lock_error)?;
        let gate = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path.join("GATE"))?;
        gate.sync_all()?;
        format::write_store(&path, &config, &space)?;
        storage::sync_directory(&path)?;
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            storage::sync_directory(parent)?;
        }
        let mut publisher = Self {
            path,
            config,
            space,
            _writer: writer,
            poisoned: false,
        };
        publisher.begin(0, PublishOptions::default())?.commit()?;
        Ok(publisher)
    }
    /// Open the publisher without changing the current immutable generation.
    /// A previous ambiguous publication is reconciled through CURRENT validation.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        storage::ensure_platform()?;
        let path = fs::canonicalize(path)?;
        let writer = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path.join("WRITER"))?;
        writer.try_lock().map_err(lock_error)?;
        let (config, space) = format::read_store(&path)?;
        // Validate the full committed generation before permitting another publication.
        let snapshot = Snapshot::open(&path)?;
        drop(snapshot);
        Ok(Self {
            path,
            config,
            space,
            _writer: writer,
            poisoned: false,
        })
    }
    /// Immutable embedding identity shared by every generation.
    pub fn space(&self) -> &EmbeddingSpace {
        &self.space
    }
    /// Construction settings carried from the source database.
    pub fn config(&self) -> &Config {
        &self.config
    }
    /// Begin a streaming replacement. No record is committed until commit succeeds.
    /// Source sequence may repeat but cannot regress. Failed/uncommitted directories
    /// remain unreachable and may be reclaimed explicitly.
    pub fn begin(
        &mut self,
        source_sequence: u64,
        options: PublishOptions,
    ) -> Result<GenerationBuilder<'_>> {
        if self.poisoned {
            return Err(Error::Poisoned);
        }
        options.validate()?;
        let previous = format::current(&self.path)?;
        if let Some((generation, crc)) = previous {
            let old = format::read_manifest(&self.path.join(generation_name(generation)), crc)?;
            if source_sequence < old.sequence {
                return Err(Error::InvalidInput(
                    "generation source sequence cannot regress".into(),
                ));
            }
        }
        let mut generation = previous.map_or(0, |(generation, _)| generation);
        let directory = loop {
            generation = generation
                .checked_add(1)
                .ok_or_else(|| Error::InvalidInput("generation counter exhausted".into()))?;
            let directory = self.path.join(generation_name(generation));
            match fs::create_dir(&directory) {
                Ok(()) => break directory,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        };
        let lease = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(directory.join("LEASE"))?;
        lease.sync_all()?;
        Ok(GenerationBuilder {
            publisher: self,
            directory,
            generation,
            sequence: source_sequence,
            options,
            entries: BTreeMap::new(),
            segments: Vec::new(),
            writer: None,
            used: 0,
            failed: false,
        })
    }
    /// Remove only non-current, unpinned generation directories.
    /// An OS reader pin survives process crashes correctly: the OS releases it on exit.
    /// Unknown files and partial directories without a lease are retained.
    pub fn collect_garbage(&mut self) -> Result<usize> {
        if self.poisoned {
            return Err(Error::Poisoned);
        }
        let gate = OpenOptions::new()
            .read(true)
            .write(true)
            .open(self.path.join("GATE"))?;
        gate.lock()?;
        let current = format::current(&self.path)?
            .ok_or_else(|| Error::Corrupt("missing CURRENT".into()))?
            .0;
        let mut removed = 0;
        for entry in fs::read_dir(&self.path)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(hex) = name.strip_prefix("gen-") else {
                continue;
            };
            if hex.len() != 16 || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
                continue;
            }
            let Ok(generation) = u64::from_str_radix(hex, 16) else {
                continue;
            };
            if name != generation_name(generation) {
                continue;
            }
            if generation == current || !entry.file_type()?.is_dir() {
                continue;
            }
            let lease = match OpenOptions::new()
                .read(true)
                .write(true)
                .open(entry.path().join("LEASE"))
            {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            match lease.try_lock() {
                Ok(()) => {
                    // GATE excludes new readers while we release the OS file handle
                    // for Windows deletion. Existing readers would have blocked try_lock.
                    drop(lease);
                    fs::remove_dir_all(entry.path())?;
                    removed += 1;
                }
                Err(std::fs::TryLockError::WouldBlock) => {}
                Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
            }
        }
        if removed != 0 {
            storage::sync_directory(&self.path)?;
        }
        Ok(removed)
    }
}

/// Streaming original records into a new immutable generation.
/// Dropping without commit leaves CURRENT unchanged.
pub struct GenerationBuilder<'a> {
    publisher: &'a mut Publisher,
    directory: PathBuf,
    generation: u64,
    sequence: u64,
    options: PublishOptions,
    entries: BTreeMap<u64, Entry>,
    segments: Vec<SegmentInfo>,
    writer: Option<stream::Writer>,
    used: usize,
    failed: bool,
}
impl GenerationBuilder<'_> {
    /// Append an original vector/payload. Duplicate IDs and invalid input abort
    /// this builder; no partially populated generation can be published.
    pub fn add(&mut self, id: u64, vector: &[f32], metadata: &str) -> Result<()> {
        if self.failed {
            return Err(Error::Poisoned);
        }
        let result = self.add_inner(id, vector, metadata);
        if result.is_err() {
            self.failed = true;
            if matches!(result, Err(Error::Io(_))) {
                self.publisher.poisoned = true;
            }
        }
        result
    }
    fn add_inner(&mut self, id: u64, vector: &[f32], metadata: &str) -> Result<()> {
        self.publisher.config.validate_vector(vector)?;
        if metadata.len() > crate::config::MAX_METADATA {
            return Err(Error::InvalidInput(
                "generation metadata exceeds 16 KiB".into(),
            ));
        }
        if self.entries.contains_key(&id) {
            return Err(Error::InvalidInput("duplicate generation ID".into()));
        }
        if self.entries.len() == self.options.max_records {
            return Err(Error::Capacity {
                resource: "generation records",
                limit: self.options.max_records,
                required: self.entries.len() + 1,
            });
        }
        let length = 12 + vector.len() * 4 + metadata.len();
        if self.writer.is_some() && self.used + length + 4 > self.options.segment_bytes {
            self.finish_segment()?;
        }
        if self.writer.is_none() {
            if self.segments.len() == MAX_SEGMENTS {
                return Err(Error::Capacity {
                    resource: "generation segments",
                    limit: MAX_SEGMENTS,
                    required: MAX_SEGMENTS + 1,
                });
            }
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.directory.join(segment_name(self.segments.len())))?;
            self.writer = Some(stream::Writer::new(
                file,
                self.options.segment_bytes,
                "generation.segment.write",
            ));
            self.used = 0;
        }
        let entry = Entry {
            id,
            segment: self.segments.len() as u32,
            offset: self.used as u32,
            metadata_len: metadata.len() as u32,
        };
        let writer = self.writer.as_mut().unwrap();
        writer.u64(id)?;
        writer.u32(metadata.len() as u32)?;
        for value in vector {
            writer.bytes(&value.to_le_bytes())?;
        }
        writer.bytes(metadata.as_bytes())?;
        self.used += length;
        self.entries.insert(id, entry);
        Ok(())
    }
    fn finish_segment(&mut self) -> Result<()> {
        if let Some(writer) = self.writer.take() {
            let (file, crc) = writer.finish()?;
            crate::storage_io::check("generation.segment.sync")?;
            file.sync_all()?;
            self.segments.push(SegmentInfo {
                length: self.used as u64 + 4,
                crc,
            });
        }
        Ok(())
    }
    /// Sync all data and publish one complete generation. A successful return is
    /// the commit acknowledgement. An I/O error can be ambiguous; reopen Publisher.
    pub fn commit(mut self) -> Result<Snapshot> {
        if self.failed || self.publisher.poisoned {
            return Err(Error::Poisoned);
        }
        let result = self.commit_inner();
        if result.is_err() {
            self.publisher.poisoned = true;
        }
        result
    }
    fn commit_inner(&mut self) -> Result<Snapshot> {
        self.finish_segment()?;
        let manifest = Manifest {
            generation: self.generation,
            sequence: self.sequence,
            config: self.publisher.config.clone(),
            space: self.publisher.space.clone(),
            segments: self.segments.clone(),
            entries: self.entries.values().copied().collect(),
        };
        let crc = format::write_manifest(&self.directory, &manifest)?;
        crate::storage_io::check("generation.directory.sync")?;
        storage::sync_directory(&self.directory)?;
        let gate = OpenOptions::new()
            .read(true)
            .write(true)
            .open(self.publisher.path.join("GATE"))?;
        gate.lock()?;
        format::publish_current(&self.publisher.path, self.generation, crc)?;
        drop(gate);
        Snapshot::open(&self.publisher.path)
    }
}

/// A pinned, immutable view. Opening never recovers or rewrites database files.
/// Refresh by opening a new Snapshot; existing views keep their original sequence.
pub struct Snapshot {
    root: PathBuf,
    manifest: Manifest,
    files: Vec<SegmentFile>,
    readers: Vec<Box<dyn SegmentReader>>,
    _pin: Arc<Pin>,
}
impl Snapshot {
    /// Pin CURRENT under the publication gate, then validate the complete immutable
    /// manifest, segment boundaries, checksums, original vectors and UTF-8.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        storage::ensure_platform()?;
        let root = fs::canonicalize(path)?;
        let (config, space) = format::read_store(&root)?;
        let gate = File::open(root.join("GATE"))?;
        gate.lock_shared()?;
        let (generation, crc) =
            format::current(&root)?.ok_or_else(|| Error::Corrupt("missing CURRENT".into()))?;
        let directory = root.join(generation_name(generation));
        let lease = File::open(directory.join("LEASE"))?;
        lease.lock_shared()?;
        let pin = Arc::new(Pin { _lease: lease });
        drop(gate);
        let manifest = format::read_manifest(&directory, crc)?;
        if manifest.generation != generation || manifest.config != config || manifest.space != space
        {
            return Err(Error::Corrupt(
                "generation identity/configuration mismatch".into(),
            ));
        }
        let mut files = Vec::with_capacity(manifest.segments.len());
        for (number, segment) in manifest.segments.iter().enumerate() {
            let file = File::open(directory.join(segment_name(number)))?;
            format::validate_segment(file.try_clone()?, &manifest, number)?;
            files.push(SegmentFile {
                file: Arc::new(file),
                length: segment.length,
                _pin: pin.clone(),
            });
        }
        let readers = files
            .iter()
            .map(|file| Box::new(file.clone()) as Box<dyn SegmentReader>)
            .collect();
        Ok(Self {
            root,
            manifest,
            files,
            readers,
            _pin: pin,
        })
    }
    /// Open CURRENT again. This view and its original-reader pin remain unchanged.
    pub fn refresh(&self) -> Result<Self> {
        let next = Self::open(&self.root)?;
        if next.space() != self.space() || next.manifest.config != self.manifest.config {
            return Err(Error::EmbeddingMismatch);
        }
        Ok(next)
    }
    /// Generation ID; independent of the source's mutation sequence.
    pub fn generation(&self) -> u64 {
        self.manifest.generation
    }
    /// Source mutation sequence captured by the publisher.
    pub fn sequence(&self) -> u64 {
        self.manifest.sequence
    }
    /// Immutable model/dimensions/metric identity.
    pub fn space(&self) -> &EmbeddingSpace {
        &self.manifest.space
    }
    /// Number of committed original records.
    pub fn len(&self) -> usize {
        self.manifest.entries.len()
    }
    /// Whether this generation has no records.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Encoded segment count.
    pub fn segment_count(&self) -> usize {
        self.manifest.segments.len()
    }
    /// Original encoded segment bytes, including payloads/checksums; not RAM/RSS.
    pub fn original_bytes(&self) -> u64 {
        self.manifest.segments.iter().map(|s| s.length).sum()
    }
    /// Stream an independently openable backup into a new generation store.
    /// Source files/reader pins remain untouched; original f32 bits and identity
    /// are retained. A failed destination is never announced as a valid backup.
    pub fn backup(&self, destination: impl AsRef<Path>, options: PublishOptions) -> Result<Self> {
        let mut publisher = Publisher::create(
            destination,
            self.manifest.config.clone(),
            self.space().clone(),
        )?;
        let mut builder = publisher.begin(self.sequence(), options)?;
        for number in 0..self.len() {
            let record = self.read_record(number)?;
            builder.add(record.id, &record.vector, &record.metadata)?;
        }
        builder.commit()
    }
    /// Iterate the original IDs in ascending order without decoding coordinates.
    pub fn ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.manifest.entries.iter().map(|e| e.id)
    }
    /// Load one original record on demand.
    pub fn get(&self, id: u64) -> Result<Option<SnapshotRecord>> {
        self.manifest
            .entries
            .binary_search_by_key(&id, |entry| entry.id)
            .map_or(Ok(None), |number| self.read_record(number).map(Some))
    }
    /// Replace positional I/O with immutable sources, such as optional mmap views.
    /// Each replacement must match the validated segment length/checksum.
    pub fn with_segment_readers<F>(mut self, mut factory: F) -> Result<Self>
    where
        F: FnMut(SegmentFile) -> io::Result<Box<dyn SegmentReader>>,
    {
        let mut readers = Vec::with_capacity(self.files.len());
        for (file, info) in self.files.iter().zip(&self.manifest.segments) {
            let reader = factory(file.clone())?;
            format::validate_source(reader.as_ref(), *info)?;
            readers.push(reader);
        }
        self.readers = readers;
        Ok(self)
    }
    fn read_record(&self, number: usize) -> Result<Record> {
        let entry = self.manifest.entries[number];
        let length = 12 + self.space().dimensions * 4 + entry.metadata_len as usize;
        let mut bytes = vec![0; length];
        self.readers[entry.segment as usize].read_exact_at(u64::from(entry.offset), &mut bytes)?;
        let mut reader = storage::Reader::new(&bytes);
        if reader.u64()? != entry.id || reader.u32()? != entry.metadata_len {
            return Err(Error::Corrupt(
                "generation record no longer matches manifest".into(),
            ));
        }
        let vector = reader.vector(self.space().dimensions)?;
        self.manifest
            .config
            .validate_vector(&vector)
            .map_err(|_| Error::Corrupt("invalid original vector".into()))?;
        let metadata = String::from_utf8(reader.take(entry.metadata_len as usize)?.to_vec())
            .map_err(|_| Error::Corrupt("invalid generation metadata UTF-8".into()))?;
        reader.finish()?;
        Ok(Record {
            id: entry.id,
            vector,
            metadata,
            deleted: false,
        })
    }
    /// Exhaustively rank original f32 vectors with the portable reference kernel.
    pub fn search_exact(&self, query: &[f32], k: usize) -> Result<GenerationSearchReport> {
        self.search_filtered(query, k, &ScalarKernel, |_, _| true)
    }
    /// Exact original-vector search using an explicit arithmetic kernel and
    /// metadata predicate. Predicates are application constraints, not authorization.
    pub fn search_filtered<F>(
        &self,
        query: &[f32],
        k: usize,
        kernel: &dyn DistanceKernel,
        mut filter: F,
    ) -> Result<GenerationSearchReport>
    where
        F: FnMut(u64, &str) -> bool,
    {
        self.validate_query(query, k)?;
        let mut heap = BinaryHeap::new();
        let mut eligible = 0;
        for number in 0..self.len() {
            let record = self.read_record(number)?;
            if !filter(record.id, &record.metadata) {
                continue;
            }
            eligible += 1;
            if k == 0 {
                continue;
            }
            let distance = original_distance(self.space().metric, query, &record.vector, kernel)?;
            keep_best(
                &mut heap,
                Ranked {
                    distance,
                    id: record.id,
                    number,
                },
                k,
            );
        }
        self.finish_search(heap, eligible, if k == 0 { 0 } else { eligible }, false, k)
    }
    fn validate_query(&self, query: &[f32], k: usize) -> Result<()> {
        self.manifest.config.validate_vector(query)?;
        if k > MAX_RESULTS {
            return Err(Error::InvalidInput("generation K exceeds 10000".into()));
        }
        Ok(())
    }
    fn finish_search(
        &self,
        heap: BinaryHeap<Ranked>,
        eligible: usize,
        work: usize,
        approximate: bool,
        k: usize,
    ) -> Result<GenerationSearchReport> {
        let mut rows = heap.into_sorted_vec();
        rows.truncate(k);
        let neighbors = rows
            .into_iter()
            .map(|row| {
                self.read_record(row.number).map(|record| Neighbor {
                    id: row.id,
                    distance: row.distance,
                    metadata: record.metadata,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(GenerationSearchReport {
            complete: neighbors.len() == k.min(eligible),
            neighbors,
            generation: self.generation(),
            sequence: self.sequence(),
            eligible_count: eligible,
            original_distance_computations: work,
            approximate,
        })
    }
    /// Build reusable derived SQ8 codes while retaining originals only in the store.
    /// Codes are an approximate candidate scan, not an HNSW graph or exact index.
    pub fn quantize(&self) -> Result<QuantizedSnapshot<'_>> {
        QuantizedSnapshot::build(self)
    }
}

/// Exact distances and count observations for a pinned generation.
#[derive(Debug)]
#[non_exhaustive]
pub struct GenerationSearchReport {
    /// Original metadata and exact reranked distances, sorted by distance then ID.
    pub neighbors: Vec<Neighbor>,
    /// Reader's immutable generation ID.
    pub generation: u64,
    /// Source sequence captured in that generation.
    pub sequence: u64,
    /// Number of eligible original records.
    pub eligible_count: usize,
    /// Original-f32 distance evaluations (excluding SQ8 code comparisons).
    pub original_distance_computations: usize,
    /// Whether min(K, eligible_count) rows were returned; never a recall guarantee.
    pub complete: bool,
    /// True when a bounded SQ8 candidate pool was selected before exact reranking.
    pub approximate: bool,
}

#[derive(Clone, Copy, Debug)]
struct Ranked {
    distance: f64,
    id: u64,
    number: usize,
}
impl PartialEq for Ranked {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Ranked {}
impl PartialOrd for Ranked {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Ranked {
    fn cmp(&self, other: &Self) -> Ordering {
        self.distance
            .total_cmp(&other.distance)
            .then(self.id.cmp(&other.id))
    }
}
fn keep_best(heap: &mut BinaryHeap<Ranked>, row: Ranked, k: usize) {
    if heap.len() < k {
        heap.push(row);
    } else if heap.peek().is_some_and(|worst| row < *worst) {
        heap.pop();
        heap.push(row);
    }
}
fn original_distance(
    metric: Metric,
    query: &[f32],
    vector: &[f32],
    kernel: &dyn DistanceKernel,
) -> Result<f64> {
    let distance = match metric {
        Metric::SquaredL2 => kernel.squared_l2(query, vector),
        Metric::InnerProduct => -kernel.dot(query, vector),
        Metric::Cosine => (1.0
            - kernel.dot(query, vector)
                * crate::math::inverse_norm(query)
                * crate::math::inverse_norm(vector))
        .clamp(0.0, 2.0),
    };
    if !distance.is_finite() {
        return Err(Error::InvalidInput(
            "distance kernel returned a non-finite result".into(),
        ));
    }
    Ok(distance)
}
fn generation_name(generation: u64) -> String {
    format!("gen-{generation:016x}")
}
fn segment_name(segment: usize) -> String {
    format!("segment-{segment:04}.bin")
}
fn lock_error(error: std::fs::TryLockError) -> Error {
    match error {
        std::fs::TryLockError::WouldBlock => Error::Locked,
        std::fs::TryLockError::Error(error) => Error::Io(error),
    }
}
