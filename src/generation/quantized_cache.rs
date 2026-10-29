//! Explicit, reconstructible SQ8 caches. Original generation files are untouched.
use super::*;
use std::{
    io::{Read, Write},
    sync::atomic::{AtomicU64, Ordering as AtomicOrdering},
};

const MAGIC: &[u8; 8] = b"VECSQ801";
const LEASE_MAGIC: &[u8; 8] = b"VECSQ8L1";
const VERSION: u32 = 1;
const MAX_CACHE_BYTES: usize = 1024 * 1024 * 1024;
const MAX_IDENTITY: usize = MAX_MANIFEST_BYTES + 1024 * 1024;

/// Bounds for an explicit prepared cache and its per-search read buffer.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct QuantizedCacheOptions {
    /// Maximum logical bytes in this managed cache directory, including pending files.
    pub max_cache_bytes: usize,
    /// Maximum code-read workspace per search, excluding results and reader indexes.
    pub buffer_bytes: usize,
}
impl Default for QuantizedCacheOptions {
    fn default() -> Self {
        Self {
            max_cache_bytes: MAX_CACHE_BYTES,
            buffer_bytes: 1024 * 1024,
        }
    }
}
impl QuantizedCacheOptions {
    /// Set a smaller disk quota; source generations are never counted or modified.
    pub fn with_max_cache_bytes(mut self, bytes: usize) -> Self {
        self.max_cache_bytes = bytes;
        self
    }
    /// Set the code workspace; it must fit one encoded row and be at most 16 MiB.
    pub fn with_buffer_bytes(mut self, bytes: usize) -> Self {
        self.buffer_bytes = bytes;
        self
    }
    fn validate(self, row_bytes: usize) -> Result<()> {
        if self.max_cache_bytes == 0
            || self.max_cache_bytes > MAX_CACHE_BYTES
            || self.buffer_bytes < row_bytes
            || self.buffer_bytes > 16 * 1024 * 1024
        {
            return Err(Error::InvalidInput(
                "invalid SQ8 cache disk/buffer bounds".into(),
            ));
        }
        Ok(())
    }
}

/// Observations from a completed explicit cache preparation.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct QuantizedCacheInfo {
    /// Original pinned generation.
    pub generation: u64,
    /// Original source sequence.
    pub sequence: u64,
    /// Complete cache file bytes, including identity, parameters and checksum.
    pub cache_bytes: usize,
    /// Logical int8 coordinate bytes, not RSS.
    pub code_bytes: usize,
    /// Logical scale/norm bytes, not RSS.
    pub parameter_bytes: usize,
}

/// A validated cache file and shared cache lease, borrowing an already validated Snapshot.
///
/// Keep the managed directory's DATA and LEASE files immutable outside these APIs.
/// Preparation/pruning takes an exclusive lease and refuses active readers. A cache
/// is optional derived data, never a replacement for original-source validation.
pub struct CachedQuantizedSnapshot<'a> {
    snapshot: &'a Snapshot,
    file: File,
    _lease: File,
    data_offset: u64,
    row_bytes: usize,
    cache_bytes: usize,
    buffer_bytes: usize,
}
impl<'a> CachedQuantizedSnapshot<'a> {
    pub(super) fn open(
        snapshot: &'a Snapshot,
        directory: &Path,
        options: QuantizedCacheOptions,
    ) -> Result<Self> {
        let identity = identity(snapshot)?;
        let (row_bytes, expected) = geometry(snapshot, identity.len(), options)?;
        let directory = resolve(snapshot, directory).map_err(read_cache_error)?;
        let lease = open_lease(&directory, false).map_err(read_cache_error)?;
        lease.try_lock_shared().map_err(lock_error)?;
        entries(&directory)?;
        let file = File::open(directory.join("DATA")).map_err(|e| read_cache_error(e.into()))?;
        if file.metadata()?.len() != expected as u64 {
            return Err(Error::Corrupt("SQ8 cache length/source mismatch".into()));
        }
        let mut reader = stream::Reader::new(file.try_clone()?, expected, expected)?;
        if reader.array::<8>()? != *MAGIC || reader.u32()? != VERSION {
            return Err(Error::Corrupt("unsupported SQ8 cache".into()));
        }
        if reader.u32()? as usize != identity.len() || reader.bytes(identity.len())? != identity {
            return Err(Error::InvalidInput(
                "SQ8 cache identity differs from this store/generation/model; prepare a new cache directory".into(),
            ));
        }
        // Validate all parameter/code invariants and the full CRC before use.
        // The workspace here is one row, not N*D codes.
        for _ in 0..snapshot.len() {
            let scale = f64::from_le_bytes(reader.array()?);
            let norm = i64::from_le_bytes(reader.array()?);
            let codes = reader.bytes(snapshot.space().dimensions)?;
            validate_row(scale, norm, &codes)?;
            if snapshot.space().metric == Metric::Cosine && norm == 0 {
                return Err(Error::Corrupt("zero cosine SQ8 code".into()));
            }
        }
        reader.finish()?;
        let rows = (options.buffer_bytes / row_bytes).min(snapshot.len());
        Ok(Self {
            snapshot,
            file,
            _lease: lease,
            data_offset: (16 + identity.len()) as u64,
            row_bytes,
            cache_bytes: expected,
            buffer_bytes: rows * row_bytes,
        })
    }
    /// Borrow the pinned original generation.
    pub fn snapshot(&self) -> &Snapshot {
        self.snapshot
    }
    /// Logical int8 coordinate bytes stored in the cache, not allocated RAM.
    pub fn code_bytes(&self) -> usize {
        self.snapshot.len() * self.snapshot.space().dimensions
    }
    /// Logical scale/norm bytes stored in the cache, not allocated RAM.
    pub fn parameter_bytes(&self) -> usize {
        self.snapshot.len() * 16
    }
    /// Complete cache file size.
    pub fn cache_bytes(&self) -> usize {
        self.cache_bytes
    }
    /// Maximum row-aligned code-read workspace for one search.
    pub fn buffer_bytes(&self) -> usize {
        self.buffer_bytes
    }

    /// Scan prepared int8 codes in bounded blocks and rerank original f32 vectors.
    pub fn search(
        &self,
        query: &[f32],
        k: usize,
        candidates: usize,
    ) -> Result<GenerationSearchReport> {
        self.search_selected(query, k, candidates, &ScalarKernel, None)
    }
    /// Use an explicit original-vector arithmetic kernel with the same code scan.
    pub fn search_with_kernel(
        &self,
        query: &[f32],
        k: usize,
        candidates: usize,
        kernel: &dyn DistanceKernel,
    ) -> Result<GenerationSearchReport> {
        self.search_selected(query, k, candidates, kernel, None)
    }
    /// Restrict candidates to eligible IDs, ignoring missing IDs and duplicates.
    pub fn search_ids(
        &self,
        query: &[f32],
        k: usize,
        candidates: usize,
        kernel: &dyn DistanceKernel,
        ids: &[u64],
    ) -> Result<GenerationSearchReport> {
        if ids.len() > MAX_RECORDS {
            return Err(Error::InvalidInput(
                "eligible ID list exceeds generation record bound".into(),
            ));
        }
        let allowed = ids
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        self.search_selected(query, k, candidates, kernel, Some(&allowed))
    }
    fn search_selected(
        &self,
        query: &[f32],
        k: usize,
        limit: usize,
        kernel: &dyn DistanceKernel,
        ids: Option<&std::collections::BTreeSet<u64>>,
    ) -> Result<GenerationSearchReport> {
        self.snapshot.validate_query(query, k)?;
        if limit < k || limit > quantization::MAX_CANDIDATES {
            return Err(Error::InvalidInput(
                "SQ8 candidate limit must be K..=65536".into(),
            ));
        }
        if k == 0 {
            let eligible = self
                .snapshot
                .manifest
                .entries
                .iter()
                .filter(|e| ids.is_none_or(|ids| ids.contains(&e.id)))
                .count();
            return self
                .snapshot
                .finish_search(BinaryHeap::new(), eligible, 0, false, 0);
        }
        let (query_codes, query_scale, query_norm) = quantization::encode(query);
        let mut candidates = BinaryHeap::new();
        let mut eligible = 0;
        self.visit_rows(|number, scale, norm, codes| {
            let entry = self.snapshot.manifest.entries[number];
            if ids.is_some_and(|ids| !ids.contains(&entry.id)) {
                return Ok(());
            }
            eligible += 1;
            let dot = dot_codes(codes, &query_codes);
            let distance = quantization::approximate_distance(
                self.snapshot.space().metric,
                scale,
                norm,
                query_scale,
                query_norm,
                dot,
            );
            if !distance.is_finite() {
                return Err(Error::Corrupt(
                    "non-finite cached candidate distance".into(),
                ));
            }
            keep_best(
                &mut candidates,
                Ranked {
                    distance,
                    id: entry.id,
                    number,
                },
                limit,
            );
            Ok(())
        })?;
        quantization::rerank(
            self.snapshot,
            query,
            k,
            candidates,
            eligible,
            kernel,
            limit < eligible,
        )
    }

    pub(super) fn visit_rows<F>(&self, mut visit: F) -> Result<()>
    where
        F: FnMut(usize, f64, i64, &[u8]) -> Result<()>,
    {
        if self.snapshot.is_empty() {
            return Ok(());
        }
        let mut buffer = vec![0; self.buffer_bytes];
        let per_block = self.buffer_bytes / self.row_bytes;
        let mut number = 0;
        while number < self.snapshot.len() {
            let rows = per_block.min(self.snapshot.len() - number);
            let bytes = &mut buffer[..rows * self.row_bytes];
            read_at(
                &self.file,
                self.data_offset + (number * self.row_bytes) as u64,
                bytes,
            )?;
            for (offset, row) in bytes.chunks_exact(self.row_bytes).enumerate() {
                let scale = f64::from_le_bytes(row[..8].try_into().unwrap());
                let norm = i64::from_le_bytes(row[8..16].try_into().unwrap());
                // Managed immutable files were validated in full at open.
                // Recheck parameter bounds without allocating row objects.
                if !scale.is_finite() || scale <= 0. || norm < 0 {
                    return Err(Error::Corrupt("SQ8 cache changed after open".into()));
                }
                visit(number + offset, scale, norm, &row[16..])?;
            }
            number += rows;
        }
        Ok(())
    }
}

#[inline]
fn dot_codes(codes: &[u8], query: &[i8]) -> i64 {
    // Validated snapshots have at most 4096 dimensions. Even the full i8
    // range has absolute partial sums <=4096*128*128=67,108,864, so i32
    // accumulation is exact. This permits portable compiler vectorization
    // without unsafe code, ISA intrinsics or any floating-point change.
    debug_assert!(codes.len() <= 4096);
    debug_assert_eq!(codes.len(), query.len());
    i64::from(
        codes
            .iter()
            .zip(query)
            .map(|(&a, &b)| i32::from(a as i8) * i32::from(b))
            .sum::<i32>(),
    )
}

fn read_cache_error(error: Error) -> Error {
    if matches!(&error, Error::Io(e) if e.kind() == std::io::ErrorKind::NotFound) {
        Error::InvalidInput(
            "missing or incomplete SQ8 cache; run snapshot-prepare-sq8 <store> <new-cache-dir> first".into(),
        )
    } else {
        error
    }
}

fn validate_row(scale: f64, norm: i64, codes: &[u8]) -> Result<()> {
    let mut actual = 0i64;
    for &code in codes {
        let code = code as i8;
        if code == i8::MIN {
            return Err(Error::Corrupt("invalid SQ8 code".into()));
        }
        actual += i64::from(code) * i64::from(code);
    }
    if !scale.is_finite() || scale <= 0. || norm != actual {
        return Err(Error::Corrupt("invalid SQ8 scale/norm".into()));
    }
    Ok(())
}

fn geometry(
    snapshot: &Snapshot,
    identity: usize,
    options: QuantizedCacheOptions,
) -> Result<(usize, usize)> {
    let row = snapshot.space().dimensions + 16;
    options.validate(row)?;
    let code = snapshot
        .len()
        .checked_mul(snapshot.space().dimensions)
        .ok_or_else(|| Error::InvalidInput("SQ8 cache size overflow".into()))?;
    if code > quantization::MAX_CODE_BYTES {
        return Err(Error::Capacity {
            resource: "SQ8 code bytes",
            limit: quantization::MAX_CODE_BYTES,
            required: code,
        });
    }
    let size = snapshot
        .len()
        .checked_mul(row)
        .and_then(|n| n.checked_add(identity + 20))
        .ok_or_else(|| Error::InvalidInput("SQ8 cache size overflow".into()))?;
    if size.saturating_add(8) > options.max_cache_bytes {
        return Err(Error::Capacity {
            resource: "SQ8 cache bytes including lease",
            limit: options.max_cache_bytes,
            required: size.saturating_add(8),
        });
    }
    Ok((row, size))
}

fn identity(snapshot: &Snapshot) -> Result<Vec<u8>> {
    let root = snapshot.root.as_os_str().as_encoded_bytes();
    if root.len() > 1024 * 1024 {
        return Err(Error::InvalidInput("SQ8 source path exceeds bound".into()));
    }
    let mut bytes = Vec::new();
    storage::u32_bytes(&mut bytes, std::env::consts::OS.len() as u32);
    bytes.extend_from_slice(std::env::consts::OS.as_bytes());
    storage::u32_bytes(&mut bytes, root.len() as u32);
    bytes.extend_from_slice(root);
    let manifest = &snapshot.manifest;
    storage::u64_bytes(&mut bytes, manifest.generation);
    storage::u64_bytes(&mut bytes, manifest.sequence);
    for value in [
        manifest.config.dimensions,
        manifest.config.m,
        manifest.config.ef_construction,
    ] {
        storage::u32_bytes(&mut bytes, value as u32);
    }
    storage::u64_bytes(&mut bytes, manifest.config.seed);
    bytes.push(match manifest.space.metric {
        Metric::SquaredL2 => 0,
        Metric::Cosine => 1,
        Metric::InnerProduct => 2,
    });
    storage::u32_bytes(&mut bytes, manifest.space.model.len() as u32);
    bytes.extend_from_slice(manifest.space.model.as_bytes());
    storage::u32_bytes(&mut bytes, manifest.segments.len() as u32);
    storage::u64_bytes(&mut bytes, manifest.entries.len() as u64);
    for segment in &manifest.segments {
        storage::u64_bytes(&mut bytes, segment.length);
        storage::u32_bytes(&mut bytes, segment.crc);
    }
    for entry in &manifest.entries {
        storage::u64_bytes(&mut bytes, entry.id);
        for value in [entry.segment, entry.offset, entry.metadata_len] {
            storage::u32_bytes(&mut bytes, value);
        }
    }
    if bytes.len() > MAX_IDENTITY {
        return Err(Error::InvalidInput(
            "SQ8 cache identity exceeds bound".into(),
        ));
    }
    Ok(bytes)
}

fn resolve(snapshot: &Snapshot, path: &Path) -> Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| Error::InvalidInput("cache requires a dedicated directory".into()))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let path = fs::canonicalize(parent)?.join(name);
    if path.starts_with(&snapshot.root) || snapshot.root.starts_with(&path) {
        return Err(Error::InvalidInput(
            "SQ8 cache must be outside the original store".into(),
        ));
    }
    if let Ok(meta) = fs::symlink_metadata(&path)
        && (!meta.is_dir() || meta.file_type().is_symlink())
    {
        return Err(Error::InvalidInput(
            "SQ8 cache must be a real dedicated directory".into(),
        ));
    }
    Ok(path)
}
fn open_lease(directory: &Path, write: bool) -> Result<File> {
    let path = directory.join("LEASE");
    let meta = fs::symlink_metadata(&path)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() != 8 {
        return Err(Error::Corrupt("not a managed SQ8 cache".into()));
    }
    let mut file = OpenOptions::new().read(true).write(write).open(path)?;
    let mut marker = [0; 8];
    file.read_exact(&mut marker)?;
    if &marker != LEASE_MAGIC {
        return Err(Error::Corrupt("not a managed SQ8 cache".into()));
    }
    Ok(file)
}
fn entries(directory: &Path) -> Result<Vec<(PathBuf, u64)>> {
    let mut result = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| Error::InvalidInput("foreign cache directory entry".into()))?;
        let pending = name.strip_prefix("DATA.pending-").is_some_and(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit() || b == b'-')
        });
        let meta = fs::symlink_metadata(entry.path())?;
        if result.len() == 1024
            || !meta.is_file()
            || meta.file_type().is_symlink()
            || !(name == "LEASE" || name == "DATA" || pending)
        {
            return Err(Error::InvalidInput(
                "foreign or excessive cache directory entries".into(),
            ));
        }
        result.push((entry.path(), meta.len()));
    }
    Ok(result)
}
struct Pending(PathBuf);
impl Drop for Pending {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub(super) fn prepare(
    snapshot: &Snapshot,
    directory: &Path,
    options: QuantizedCacheOptions,
) -> Result<QuantizedCacheInfo> {
    let identity = identity(snapshot)?;
    let (_, expected) = geometry(snapshot, identity.len(), options)?;
    let directory = resolve(snapshot, directory)?;
    match fs::create_dir(&directory) {
        Ok(()) => {
            let mut lease = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join("LEASE"))?;
            lease.write_all(LEASE_MAGIC)?;
            lease.sync_all()?;
            storage::sync_directory(&directory)?;
            storage::sync_directory(directory.parent().unwrap())?;
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let lease = open_lease(&directory, true)?;
    lease.try_lock().map_err(lock_error)?;
    let existing = entries(&directory)?;
    if directory.join("DATA").try_exists()? {
        return Err(Error::AlreadyExists);
    }
    let used = existing
        .iter()
        .try_fold(0usize, |sum, (_, len)| {
            usize::try_from(*len).ok().and_then(|n| sum.checked_add(n))
        })
        .ok_or_else(|| Error::InvalidInput("SQ8 directory size overflow".into()))?;
    let required = used
        .checked_add(expected)
        .ok_or_else(|| Error::InvalidInput("SQ8 directory size overflow".into()))?;
    if required > options.max_cache_bytes {
        return Err(Error::Capacity {
            resource: "SQ8 cache directory bytes",
            limit: options.max_cache_bytes,
            required,
        });
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let (pending, file) = {
        let mut chosen = None;
        for _ in 0..1024 {
            let path = directory.join(format!(
                "DATA.pending-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, AtomicOrdering::Relaxed)
            ));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    chosen = Some((Pending(path), file));
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        chosen.ok_or_else(|| Error::InvalidInput("too many pending cache names".into()))?
    };
    let mut writer = stream::Writer::new(file, expected, "quantized_cache.write");
    writer.bytes(MAGIC)?;
    writer.u32(VERSION)?;
    writer.u32(identity.len() as u32)?;
    writer.bytes(&identity)?;
    for number in 0..snapshot.len() {
        let record = snapshot.read_record(number)?;
        let (codes, scale, norm) = quantization::encode(&record.vector);
        writer.bytes(&scale.to_le_bytes())?;
        writer.bytes(&norm.to_le_bytes())?;
        let raw: Vec<_> = codes.into_iter().map(|code| code as u8).collect();
        writer.bytes(&raw)?;
    }
    let (file, _) = writer.finish()?;
    crate::storage_io::check("quantized_cache.sync")?;
    file.sync_all()?;
    drop(file);
    crate::storage_io::check("quantized_cache.publish")?;
    fs::rename(&pending.0, directory.join("DATA"))?;
    crate::storage_io::check("quantized_cache.directory.sync")?;
    storage::sync_directory(&directory)?;
    Ok(QuantizedCacheInfo {
        generation: snapshot.generation(),
        sequence: snapshot.sequence(),
        cache_bytes: expected,
        code_bytes: snapshot.len() * snapshot.space().dimensions,
        parameter_bytes: snapshot.len() * 16,
    })
}

/// Remove only derived DATA/pending files in a managed SQ8 cache directory.
///
/// Refuses active readers/preparers and foreign entries. The stable LEASE marker
/// and directory remain, preventing lock-inode replacement races on reuse.
/// Source generations are never removed. An I/O error may follow partial cleanup.
pub fn remove_quantized_cache(directory: impl AsRef<Path>) -> Result<usize> {
    let directory = directory.as_ref();
    let meta = fs::symlink_metadata(directory)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(Error::InvalidInput(
            "invalid managed cache directory".into(),
        ));
    }
    let lease = open_lease(directory, true)?;
    lease.try_lock().map_err(lock_error)?;
    let entries = entries(directory)?;
    let mut removed = 0;
    for (path, _) in entries {
        if path.file_name() != Some(std::ffi::OsStr::new("LEASE")) {
            crate::storage_io::check("quantized_cache.prune")?;
            fs::remove_file(path)?;
            removed += 1;
        }
    }
    storage::sync_directory(directory)?;
    Ok(removed)
}

fn read_at(file: &File, mut offset: u64, mut destination: &mut [u8]) -> io::Result<()> {
    while !destination.is_empty() {
        #[cfg(unix)]
        let read = {
            use std::os::unix::fs::FileExt;
            file.read_at(destination, offset)
        };
        #[cfg(windows)]
        let read = {
            use std::os::windows::fs::FileExt;
            file.seek_read(destination, offset)
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_integer_dot_matches_i64_reference_including_extreme_dimensions() {
        for a in -128i16..=127 {
            for b in -128i16..=127 {
                assert_eq!(
                    dot_codes(&[a as u8], &[b as i8]),
                    i64::from(a) * i64::from(b)
                );
            }
        }
        for length in [1, 3, 17, 768, 1536, 4095, 4096] {
            for (a, b) in [(-128i8, -128i8), (-128, 127), (127, 127), (0, -128)] {
                let codes = vec![a as u8; length];
                let query = vec![b; length];
                assert_eq!(
                    dot_codes(&codes, &query),
                    i64::from(a) * i64::from(b) * length as i64
                );
            }
            let codes: Vec<_> = (0..length).map(|n| n as u8).collect();
            let query: Vec<_> = (0..length).map(|n| (n * 37) as i8).collect();
            let reference = codes
                .iter()
                .zip(&query)
                .map(|(&a, &b)| i64::from(a as i8) * i64::from(b))
                .sum::<i64>();
            assert_eq!(dot_codes(&codes, &query), reference);
        }
    }
    use crate::storage_io::faults::{Guard, Mode};
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "vecnook-sq8-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, AtomicOrdering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn snapshot(path: &Path) -> Snapshot {
        let mut publisher = Publisher::create(
            path,
            Config::new(3),
            EmbeddingSpace::new("cache", 3, Metric::SquaredL2),
        )
        .unwrap();
        let mut builder = publisher.begin(9, PublishOptions::default()).unwrap();
        builder
            .add(u64::MAX, &[1., -0., f32::from_bits(1)], "원문")
            .unwrap();
        builder.add(1, &[-3., 2., 1.], "other").unwrap();
        builder.commit().unwrap()
    }
    #[test]
    fn streamed_codes_parameters_and_identity_match_original_encoder() {
        let temp = Temp::new();
        let mut source = snapshot(&temp.0.join("store"));
        let path = temp.0.join("cache");
        let info = prepare(&source, &path, QuantizedCacheOptions::default()).unwrap();
        let cached = CachedQuantizedSnapshot::open(
            &source,
            &path,
            QuantizedCacheOptions::default().with_buffer_bytes(20),
        )
        .unwrap();
        assert_eq!(info.cache_bytes, cached.cache_bytes());
        assert_eq!(cached.buffer_bytes(), 19);
        cached
            .visit_rows(|n, scale, norm, raw| {
                let vector = source.read_record(n)?.vector;
                let (expected, s, t) = quantization::encode(&vector);
                assert_eq!(scale.to_bits(), s.to_bits());
                assert_eq!(norm, t);
                assert_eq!(raw.iter().map(|&b| b as i8).collect::<Vec<_>>(), expected);
                Ok(())
            })
            .unwrap();
        drop(cached);
        let before = identity(&source).unwrap();
        macro_rules! changed {
            ($field:expr,$value:expr) => {{
                let original = $field.clone();
                $field = $value;
                assert_ne!(before, identity(&source).unwrap());
                assert!(
                    CachedQuantizedSnapshot::open(&source, &path, QuantizedCacheOptions::default())
                        .is_err()
                );
                $field = original;
            }};
        }
        changed!(source.manifest.sequence, 10);
        changed!(source.manifest.generation, 99);
        changed!(source.manifest.space.model, "other".to_owned());
        changed!(source.manifest.space.metric, Metric::InnerProduct);
        changed!(source.manifest.config.dimensions, 4);
        changed!(source.manifest.segments[0].crc, 0);
        changed!(source.manifest.entries[0].id, 999);
    }
    #[test]
    fn every_cache_write_boundary_is_fail_closed_and_recoverable() {
        for (point, mode) in [
            ("quantized_cache.write", Mode::Partial(7)),
            ("quantized_cache.sync", Mode::Fail),
            ("quantized_cache.publish", Mode::Fail),
            ("quantized_cache.directory.sync", Mode::Fail),
        ] {
            let temp = Temp::new();
            let source = snapshot(&temp.0.join("store"));
            let path = temp.0.join("cache");
            let guard = Guard::new(point, mode);
            assert!(
                prepare(&source, &path, QuantizedCacheOptions::default()).is_err(),
                "{point}"
            );
            drop(guard);
            let reopened = Snapshot::open(&source.root).unwrap();
            assert_eq!(
                reopened.get(u64::MAX).unwrap().unwrap().vector[1].to_bits(),
                (-0.0f32).to_bits()
            );
            if point == "quantized_cache.directory.sync" {
                assert!(
                    CachedQuantizedSnapshot::open(&source, &path, QuantizedCacheOptions::default())
                        .is_ok()
                );
            } else {
                assert!(!path.join("DATA").exists());
                prepare(&source, &path, QuantizedCacheOptions::default()).unwrap();
                assert!(
                    CachedQuantizedSnapshot::open(&source, &path, QuantizedCacheOptions::default())
                        .is_ok()
                );
            }
        }
    }
    #[test]
    fn fragmented_cache_writes_preserve_complete_rows_and_crc() {
        for mode in [Mode::Short, Mode::Interrupted] {
            let temp = Temp::new();
            let source = snapshot(&temp.0.join("store"));
            let path = temp.0.join("cache");
            let guard = Guard::new("quantized_cache.write", mode);
            prepare(&source, &path, QuantizedCacheOptions::default()).unwrap();
            drop(guard);
            CachedQuantizedSnapshot::open(&source, &path, QuantizedCacheOptions::default())
                .unwrap();
        }
    }
    #[test]
    fn invalid_parameters_with_a_valid_crc_still_fail() {
        let temp = Temp::new();
        let source = snapshot(&temp.0.join("store"));
        let path = temp.0.join("cache");
        prepare(&source, &path, QuantizedCacheOptions::default()).unwrap();
        let original = fs::read(path.join("DATA")).unwrap();
        let header = u32::from_le_bytes(original[12..16].try_into().unwrap()) as usize;
        for (offset, value) in [
            (16 + header, f64::NAN.to_le_bytes()),
            (24 + header, (-1i64).to_le_bytes()),
        ] {
            let mut bytes = original.clone();
            bytes[offset..offset + 8].copy_from_slice(&value);
            let end = bytes.len() - 4;
            let crc = storage::crc32(&bytes[..end]);
            bytes[end..].copy_from_slice(&crc.to_le_bytes());
            fs::write(path.join("DATA"), bytes).unwrap();
            assert!(
                CachedQuantizedSnapshot::open(&source, &path, QuantizedCacheOptions::default())
                    .is_err()
            );
        }
    }
}
