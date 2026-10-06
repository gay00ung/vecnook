use crate::{
    BatchReport, Config, Database, Document, Error, IndexStats, Metric, Mutation, RecoveryInfo,
    Result, SearchOptions, SearchReport,
    document::{PayloadReader, corrupt, put_string},
    storage,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
};

/// Identity of a compatible embedding space. Include model digest and prompt
/// convention in `model`; equal dimensions alone do not imply compatibility.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct EmbeddingSpace {
    /// Nonempty application-defined model/version/prompt identity, at most 512 UTF-8 bytes.
    pub model: String,
    /// Fixed vector dimensions in 1..=4096.
    pub dimensions: usize,
    /// Distance metric expected by this embedding model.
    pub metric: Metric,
}
impl EmbeddingSpace {
    /// Build an identity; creation/open validates its bounds.
    pub fn new(model: impl Into<String>, dimensions: usize, metric: Metric) -> Self {
        Self {
            model: model.into(),
            dimensions,
            metric,
        }
    }
    /// Validate identity bounds before creating or opening a collection.
    pub fn validate(&self) -> Result<()> {
        if self.model.is_empty() || self.model.len() > 512 || self.model.contains('\0') {
            return Err(Error::InvalidInput(
                "model identity must contain 1..512 UTF-8 bytes without NUL".into(),
            ));
        }
        Config::new(self.dimensions)
            .with_metric(self.metric)
            .validate()
    }
}

/// Indexed source equality and all-tag intersection. An empty filter is unfiltered.
#[derive(Clone, Copy, Debug, Default)]
#[non_exhaustive]
pub struct DocumentFilter<'a> {
    /// Exact original source identifier, if specified.
    pub source: Option<&'a str>,
    /// Every listed tag must be present; duplicate filter tags have no effect.
    pub tags: &'a [&'a str],
}

impl<'a> DocumentFilter<'a> {
    /// Select exact source equality; tags, if any, must also match.
    pub fn with_source(mut self, source: &'a str) -> Self {
        self.source = Some(source);
        self
    }
    /// Select an optional source, useful for application request adapters.
    pub fn with_optional_source(mut self, source: Option<&'a str>) -> Self {
        self.source = source;
        self
    }
    /// Require every listed tag.
    pub fn with_tags(mut self, tags: &'a [&'a str]) -> Self {
        self.tags = tags;
        self
    }
}

/// Borrowed document mutations in an atomic ordered collection batch.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum DocumentMutation<'a> {
    /// Insert or replace a complete chunk and its vector.
    Put {
        /// Typed original text, source, line range and tags.
        document: &'a Document,
        /// Finite coordinates in this collection's embedding space.
        vector: &'a [f32],
    },
    /// Delete a document ID; missing IDs are a no-op.
    Delete {
        /// Application document ID.
        id: u64,
    },
}

/// A matched original document and its distance.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct DocumentNeighbor {
    /// Decoded chunk with source and tags.
    pub document: Document,
    /// Ascending distance in the collection metric.
    pub distance: f64,
}
/// A typed result plus the underlying execution/work report.
#[derive(Debug)]
#[non_exhaustive]
pub struct DocumentSearchReport {
    /// Original documents in distance/ID order.
    pub neighbors: Vec<DocumentNeighbor>,
    /// Engine decisions and counters; raw neighbors contain versioned document payloads.
    pub search: SearchReport,
}

type PostingMap = BTreeMap<String, BTreeSet<u64>>;
#[derive(Default)]
struct Postings {
    sources: PostingMap,
    tags: PostingMap,
}
impl Postings {
    fn add(&mut self, id: u64, source: &str, tags: &[String]) {
        self.sources
            .entry(source.to_owned())
            .or_default()
            .insert(id);
        for tag in tags {
            self.tags.entry(tag.clone()).or_default().insert(id);
        }
    }
    fn remove(&mut self, id: u64, source: &str, tags: &[String]) {
        fn remove_key(map: &mut PostingMap, key: &str, id: u64) {
            if let Some(set) = map.get_mut(key) {
                set.remove(&id);
                if set.is_empty() {
                    map.remove(key);
                }
            }
        }
        remove_key(&mut self.sources, source, id);
        for tag in tags {
            remove_key(&mut self.tags, tag, id);
        }
    }
    fn from_db(db: &Database) -> Result<Self> {
        let mut result = Self::default();
        for record in db.iter() {
            let doc = Document::from_payload(&record.metadata)?;
            if doc.id != record.id {
                return Err(corrupt("document ID differs from record ID"));
            }
            result.add(doc.id, &doc.source, &doc.tags);
        }
        Ok(result)
    }
    fn select(&self, filter: DocumentFilter<'_>) -> Vec<u64> {
        let mut sets = Vec::new();
        if let Some(source) = filter.source {
            let Some(set) = self.sources.get(source) else {
                return Vec::new();
            };
            sets.push(set);
        }
        for tag in filter.tags {
            let Some(set) = self.tags.get(*tag) else {
                return Vec::new();
            };
            sets.push(set);
        }
        let Some(smallest) = sets.iter().min_by_key(|set| set.len()) else {
            return Vec::new();
        };
        smallest
            .iter()
            .copied()
            .filter(|id| sets.iter().all(|set| set.contains(id)))
            .collect()
    }
}

/// Named document database under an application-owned root. Each collection is
/// independently locked and persisted; tag/source indexes are rebuilt on open.
/// The immutable checksummed `collection.bin` binds name and embedding space.
pub struct Collection {
    db: Database,
    name: String,
    space: EmbeddingSpace,
    postings: Postings,
}
impl Collection {
    /// Create a fresh named collection. Model identity is immutable; dimensions
    /// and metric are taken from config. Failure can leave an incomplete directory.
    pub fn create(root: impl AsRef<Path>, name: &str, config: Config, model: &str) -> Result<Self> {
        let space = EmbeddingSpace::new(model, config.dimensions, config.metric);
        space.validate()?;
        let path = collection_path(root.as_ref(), name)?;
        if path.exists() {
            return Err(Error::AlreadyExists);
        }
        let db = Database::create(&path, config)?;
        write_header(&path, name, &space)?;
        Ok(Self {
            db,
            name: name.to_owned(),
            space,
            postings: Postings::default(),
        })
    }
    /// Open and validate the expected model identity, dimensions and metric.
    /// A mismatch returns EmbeddingMismatch before accepting any document/query.
    /// Malformed payloads or inconsistent headers return Corrupt.
    pub fn open(root: impl AsRef<Path>, name: &str, expected: &EmbeddingSpace) -> Result<Self> {
        expected.validate()?;
        let path = collection_path(root.as_ref(), name)?;
        let space = read_header(&path, name)?;
        if &space != expected {
            return Err(Error::EmbeddingMismatch);
        }
        let db = Database::open(&path)?;
        if db.config().dimensions != space.dimensions || db.config().metric != space.metric {
            return Err(corrupt("collection header/config mismatch"));
        }
        let postings = Postings::from_db(&db)?;
        Ok(Self {
            db,
            name: name.to_owned(),
            space,
            postings,
        })
    }
    /// Inspect immutable identity without opening/locking the vector database.
    /// This checks the header only; `open` validates the database and its records.
    pub fn describe(root: impl AsRef<Path>, name: &str) -> Result<EmbeddingSpace> {
        read_header(&collection_path(root.as_ref(), name)?, name)
    }
    /// Collection name, a bounded simple path component.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Stored model identity, dimensionality and metric.
    pub fn space(&self) -> &EmbeddingSpace {
        &self.space
    }
    /// Number of active document chunks.
    pub fn len(&self) -> usize {
        self.db.len()
    }
    /// Whether no documents are active.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Record/graph counts; includes tombstoned document versions.
    pub fn stats(&self) -> IndexStats {
        self.db.stats()
    }
    /// Estimated snapshot and physical-node headroom, including old versions.
    pub fn capacity(&self) -> crate::CapacityStatus {
        self.db.capacity()
    }
    /// Export original documents/vectors and embedding identity to a new file.
    pub fn export(&self, destination: impl AsRef<Path>) -> Result<()> {
        self.db.export_collection(destination.as_ref(), &self.space)
    }
    /// Validate and import a logical document export into a new named directory.
    /// All documents are validated before claiming the destination. A storage failure
    /// can leave an incomplete directory; it is never overwritten on retry.
    pub fn import(source: impl AsRef<Path>, root: impl AsRef<Path>, name: &str) -> Result<Self> {
        let data = crate::transfer::read(source.as_ref())?;
        let space = data.space.ok_or_else(|| {
            Error::InvalidInput("document import requires a collection export".into())
        })?;
        let index = crate::VectorIndex::from_records(data.config, data.records)?;
        let path = collection_path(root.as_ref(), name)?;
        crate::transfer::claim_directory(&path)?;
        let db = Database::create_from_index(&path, index)?;
        write_header(&path, name, &space)?;
        let postings = Postings::from_db(&db)?;
        Ok(Self {
            db,
            name: name.to_owned(),
            space,
            postings,
        })
    }
    /// Recovery observations from the underlying database open.
    pub fn recovery_info(&self) -> &RecoveryInfo {
        self.db.recovery_info()
    }
    /// Current committed WAL sequence, for optimistic write preconditions.
    pub fn sequence(&self) -> u64 {
        self.db.sequence()
    }
    /// Decode active documents in ascending ID order while borrowing this collection.
    pub fn documents(&self) -> impl Iterator<Item = Result<Document>> + '_ {
        self.db
            .iter()
            .map(|record| Document::from_payload(&record.metadata))
    }
    /// Commit only if no write occurred after the caller observed `expected`.
    /// A conflict never changes RAM, WAL or derived postings.
    pub fn write_batch_if_sequence(
        &mut self,
        expected: u64,
        operations: &[DocumentMutation<'_>],
    ) -> Result<BatchReport> {
        if self.sequence() != expected {
            return Err(Error::Conflict {
                expected,
                actual: self.sequence(),
            });
        }
        self.write_batch(operations)
    }
    /// Borrow original active coordinates without decoding the text payload.
    pub fn vector(&self, id: u64) -> Option<&[f32]> {
        self.db.get(id).map(|r| r.vector.as_slice())
    }
    /// Decode an active original document; returns None for absent/deleted IDs.
    pub fn get(&self, id: u64) -> Result<Option<Document>> {
        self.db
            .get(id)
            .map(|r| Document::from_payload(&r.metadata))
            .transpose()
    }
    /// Durably insert/replace a document and update derived source/tag postings.
    /// Returns true for a newly active ID. Input errors change nothing; I/O failures
    /// have the same ambiguous persistence/poisoning contract as Database.
    pub fn put(&mut self, document: &Document, vector: &[f32]) -> Result<bool> {
        let report = self.write_batch(&[DocumentMutation::Put { document, vector }])?;
        Ok(report.inserted == 1)
    }
    /// Durably delete a document; false means already absent.
    pub fn delete(&mut self, id: u64) -> Result<bool> {
        Ok(self
            .write_batch(&[DocumentMutation::Delete { id }])?
            .deleted
            == 1)
    }
    /// Validate all documents before committing one atomic ordered WAL frame.
    /// Enforces the database's 1024-operation/8 MiB bounds. Derived indexes are
    /// changed only after success and observe same-ID operation ordering.
    pub fn write_batch(&mut self, operations: &[DocumentMutation<'_>]) -> Result<BatchReport> {
        if operations.len() > 1024 {
            return Err(Error::Capacity {
                resource: "batch_operations",
                limit: 1024,
                required: operations.len(),
            });
        }
        let mut states: BTreeMap<u64, Option<(String, Vec<String>)>> = BTreeMap::new();
        let mut payloads = Vec::with_capacity(operations.len());
        for operation in operations {
            let id = match operation {
                DocumentMutation::Put { document, .. } => document.id,
                DocumentMutation::Delete { id } => *id,
            };
            if let std::collections::btree_map::Entry::Vacant(entry) = states.entry(id) {
                entry.insert(self.get(id)?.map(|doc| (doc.source, doc.tags)));
            }
            payloads.push(match operation {
                DocumentMutation::Put { document, .. } => document.to_payload()?,
                DocumentMutation::Delete { .. } => String::new(),
            });
        }
        let mutations: Vec<_> = operations
            .iter()
            .zip(&payloads)
            .map(|(op, payload)| match op {
                DocumentMutation::Put { document, vector } => Mutation::Put {
                    id: document.id,
                    vector,
                    metadata: payload,
                },
                DocumentMutation::Delete { id } => Mutation::Delete { id: *id },
            })
            .collect();
        let report = self.db.write_batch(&mutations)?;
        for operation in operations {
            let id = match operation {
                DocumentMutation::Put { document, .. } => document.id,
                DocumentMutation::Delete { id } => *id,
            };
            if let Some((source, tags)) = states.get_mut(&id).unwrap().take() {
                self.postings.remove(id, &source, &tags);
            }
            if let DocumentMutation::Put { document, .. } = operation {
                self.postings.add(id, &document.source, &document.tags);
                states.insert(id, Some((document.source.clone(), document.tags.clone())));
            }
        }
        Ok(report)
    }
    /// Search original chunks. Source and all-tag intersections use posting lists;
    /// an empty filter takes the fast unfiltered database path.
    pub fn search(
        &self,
        query: &[f32],
        k: usize,
        options: SearchOptions,
        filter: DocumentFilter<'_>,
    ) -> Result<DocumentSearchReport> {
        if filter.tags.len() > 32 {
            return Err(Error::InvalidInput("filter exceeds 32 tags".into()));
        }
        let search = if filter.source.is_none() && filter.tags.is_empty() {
            self.db.search(query, k, options)?
        } else {
            self.db
                .search_ids(query, k, options, &self.postings.select(filter))?
        };
        let neighbors = search
            .neighbors
            .iter()
            .map(|n| {
                Ok(DocumentNeighbor {
                    document: Document::from_payload(&n.metadata)?,
                    distance: n.distance,
                })
            })
            .collect::<Result<_>>()?;
        Ok(DocumentSearchReport { neighbors, search })
    }
    /// Persist a checkpoint and bound WAL replay work.
    pub fn checkpoint(&mut self) -> Result<()> {
        self.db.checkpoint()
    }
    /// Reclaim tombstones; active IDs and derived source/tag indexes are preserved.
    pub fn compact(&mut self) -> Result<usize> {
        self.db.compact()
    }
    /// Create an independently openable named backup, including embedding identity.
    /// Refuses an existing destination; failure can leave an incomplete directory.
    pub fn backup(&mut self, root: impl AsRef<Path>, name: &str) -> Result<()> {
        let path = collection_path(root.as_ref(), name)?;
        self.db.backup(&path)?;
        write_header(&path, name, &self.space)
    }
    /// Validate the graph and independently rebuild/compare source/tag postings.
    pub fn check_invariants(&self) -> Result<()> {
        self.db.check_invariants()?;
        let expected = Postings::from_db(&self.db)?;
        if expected.sources != self.postings.sources || expected.tags != self.postings.tags {
            return Err(corrupt("document posting mismatch"));
        }
        Ok(())
    }
}

fn collection_path(root: &Path, name: &str) -> Result<PathBuf> {
    let upper = name.to_ascii_uppercase();
    let reserved = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        || reserved.contains(&upper.as_str())
    {
        return Err(Error::InvalidInput("collection name must be 1..64 ASCII letters/digits/hyphens/underscores and not a reserved device name".into()));
    }
    let path = root.join(name);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(Error::InvalidInput(
                "symlink collection directory rejected".into(),
            ));
        }
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
        _ => (),
    }
    Ok(path)
}
fn write_header(path: &Path, name: &str, space: &EmbeddingSpace) -> Result<()> {
    let mut bytes = b"VCOLL001".to_vec();
    put_string(&mut bytes, name);
    put_string(&mut bytes, &space.model);
    bytes.extend_from_slice(&(space.dimensions as u32).to_le_bytes());
    bytes.push(match space.metric {
        Metric::SquaredL2 => 0,
        Metric::Cosine => 1,
        Metric::InnerProduct => 2,
    });
    bytes.extend_from_slice(&storage::crc32(&bytes).to_le_bytes());
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path.join("collection.bin"))?;
    crate::storage_io::write(&mut file, &bytes, "collection.write")?;
    crate::storage_io::check("collection.sync")?;
    file.sync_all()?;
    storage::sync_directory(path)
}
pub(crate) fn read_header(path: &Path, name: &str) -> Result<EmbeddingSpace> {
    let mut bytes = Vec::new();
    File::open(path.join("collection.bin"))?
        .take(1025)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 || bytes.len() < 25 || !bytes.starts_with(b"VCOLL001") {
        return Err(corrupt("collection header size/version"));
    }
    let end = bytes.len() - 4;
    if storage::crc32(&bytes[..end]) != u32::from_le_bytes(bytes[end..].try_into().unwrap()) {
        return Err(corrupt("collection header checksum"));
    }
    let mut reader = PayloadReader::new(&bytes[8..end]);
    if reader.string(64)? != name {
        return Err(corrupt("collection header name mismatch"));
    }
    let model = reader.string(512)?;
    let dimensions = reader.u32()? as usize;
    let metric = match reader.take(1)?[0] {
        0 => Metric::SquaredL2,
        1 => Metric::Cosine,
        2 => Metric::InnerProduct,
        _ => return Err(corrupt("collection metric")),
    };
    reader.finish()?;
    let space = EmbeddingSpace::new(model, dimensions, metric);
    space.validate().map_err(|e| corrupt(&e.to_string()))?;
    Ok(space)
}
