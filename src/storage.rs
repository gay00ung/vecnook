use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{BufReader, Read, Seek, SeekFrom, Write},
    path::Path,
};

use crate::{
    Config, Error, Metric, Mutation, Record, Result, VectorIndex,
    config::{
        MAX_BATCH_BYTES, MAX_BATCH_OPERATIONS, MAX_METADATA, MAX_RECORDS, MAX_SNAPSHOT_BYTES,
        SNAPSHOT_HEADER_BYTES,
    },
    graph,
    index::GraphState,
};

const MAGIC: &[u8; 8] = b"VECTORS1";
const VERSION: u32 = 2;
const MAX_FRAME: usize = MAX_BATCH_BYTES;

const fn crc_table() -> [u32; 256] {
    let mut table = [0; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut bit = 0;
        while bit < 8 {
            c = if c & 1 == 1 {
                0xedb88320 ^ (c >> 1)
            } else {
                c >> 1
            };
            bit += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}
const CRC_TABLE: [u32; 256] = crc_table();

pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc = CRC_TABLE[((crc ^ u32::from(byte)) & 255) as usize] ^ (crc >> 8);
    }
    !crc
}

pub(crate) fn u32_bytes(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}
pub(crate) fn u64_bytes(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    pub(crate) fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }
    pub(crate) fn take(&mut self, length: usize) -> Result<&'a [u8]> {
        if length > self.remaining() {
            return Err(Error::Corrupt("truncated field".into()));
        }
        let start = self.offset;
        self.offset += length;
        Ok(&self.bytes[start..self.offset])
    }
    pub(crate) fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub(crate) fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub(crate) fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub(crate) fn vector(&mut self, dimensions: usize) -> Result<Vec<f32>> {
        let raw = self.take(dimensions * 4)?;
        let values: Vec<_> = raw
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&b| f32::from_le_bytes(b))
            .collect();
        if values.iter().any(|v| !v.is_finite()) {
            return Err(Error::Corrupt("non-finite vector coordinate".into()));
        }
        Ok(values)
    }
    fn metadata(&mut self) -> Result<String> {
        let length = self.u32()? as usize;
        if length > MAX_METADATA {
            return Err(Error::Corrupt("metadata length exceeds limit".into()));
        }
        String::from_utf8(self.take(length)?.to_vec())
            .map_err(|_| Error::Corrupt("invalid metadata UTF-8".into()))
    }
    pub(crate) fn finish(&self) -> Result<()> {
        if self.remaining() == 0 {
            Ok(())
        } else {
            Err(Error::Corrupt("unexpected trailing fields".into()))
        }
    }
}

pub(crate) fn ensure_platform() -> Result<()> {
    if cfg!(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows"
    )) {
        Ok(())
    } else {
        Err(Error::UnsupportedPlatform)
    }
}

pub(crate) fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    let directory = {
        use std::os::windows::fs::OpenOptionsExt;
        // CreateFileW needs BACKUP_SEMANTICS for a directory handle, and
        // FlushFileBuffers needs GENERIC_WRITE. Propagate either failure;
        // clearing the WAL must never silently skip this synchronization.
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x02000000;
        OpenOptions::new()
            .write(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)?
    };
    #[cfg(not(target_os = "windows"))]
    let directory = File::open(path)?;
    directory.sync_all()?;
    Ok(())
}

pub(crate) fn acquire_lock(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.join("LOCK"))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(std::fs::TryLockError::WouldBlock) => Err(Error::Locked),
        Err(std::fs::TryLockError::Error(error)) => Err(Error::Io(error)),
    }
}

pub(crate) fn write_snapshot(path: &Path, index: &VectorIndex, sequence: u64) -> Result<()> {
    let config = index.config();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    u32_bytes(&mut bytes, VERSION);
    u32_bytes(&mut bytes, config.dimensions as u32);
    u32_bytes(&mut bytes, config.m as u32);
    u32_bytes(&mut bytes, config.ef_construction as u32);
    u32_bytes(
        &mut bytes,
        match config.metric {
            Metric::SquaredL2 => 0,
            Metric::Cosine => 1,
            Metric::InnerProduct => 2,
        },
    );
    u64_bytes(&mut bytes, config.seed);
    u64_bytes(&mut bytes, sequence);
    u64_bytes(&mut bytes, index.stats().physical_nodes as u64);
    for record in index.records() {
        u64_bytes(&mut bytes, record.id);
        bytes.push(u8::from(record.deleted));
        for value in &record.vector {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        u32_bytes(&mut bytes, record.metadata.len() as u32);
        bytes.extend_from_slice(record.metadata.as_bytes());
    }
    if bytes.len() + 4 > MAX_SNAPSHOT_BYTES {
        return Err(Error::InvalidInput("snapshot size limit exceeded".into()));
    }
    let checksum = crc32(&bytes);
    u32_bytes(&mut bytes, checksum);
    let temporary = path.join("snapshot.tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path.join("snapshot.bin"))?;
    sync_directory(path)?;
    graph::write(path, index, sequence, checksum)
}

struct Snapshot {
    config: Config,
    sequence: u64,
    records: Vec<Record>,
    checksum: u32,
}

fn read_snapshot(path: &Path) -> Result<Snapshot> {
    let file = File::open(path.join("snapshot.bin"))?;
    if file.metadata()?.len() > MAX_SNAPSHOT_BYTES as u64 {
        return Err(Error::Corrupt("snapshot exceeds size limit".into()));
    }
    let mut bytes = Vec::new();
    file.take(MAX_SNAPSHOT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() < 48 + 4 || bytes.len() > MAX_SNAPSHOT_BYTES {
        return Err(Error::Corrupt("invalid snapshot length".into()));
    }
    let checksum_offset = bytes.len() - 4;
    let expected = u32::from_le_bytes(bytes[checksum_offset..].try_into().unwrap());
    if crc32(&bytes[..checksum_offset]) != expected {
        return Err(Error::Corrupt("snapshot checksum mismatch".into()));
    }
    let mut reader = Reader::new(&bytes[..checksum_offset]);
    if reader.take(8)? != MAGIC {
        return Err(Error::Corrupt("unknown snapshot magic/version".into()));
    }
    let version = reader.u32()?;
    if !matches!(version, 1 | 2) {
        return Err(Error::Corrupt("unknown snapshot version".into()));
    }
    let mut config = Config {
        dimensions: reader.u32()? as usize,
        m: reader.u32()? as usize,
        ef_construction: reader.u32()? as usize,
        seed: 0,
        metric: Metric::SquaredL2,
    };
    if version == 2 {
        config.metric = match reader.u32()? {
            0 => Metric::SquaredL2,
            1 => Metric::Cosine,
            2 => Metric::InnerProduct,
            _ => return Err(Error::Corrupt("unknown distance metric".into())),
        };
    }
    config.seed = reader.u64()?;
    config
        .validate()
        .map_err(|e| Error::Corrupt(e.to_string()))?;
    let sequence = reader.u64()?;
    let count = reader.u64()?;
    let minimum_record = 13 + config.dimensions * 4;
    if count > MAX_RECORDS as u64
        || count > (reader.remaining() / minimum_record) as u64
        || (version == 1 && count > sequence)
    {
        return Err(Error::Corrupt("invalid snapshot record count".into()));
    }
    let mut records = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let id = reader.u64()?;
        let deleted = match reader.u8()? {
            0 => false,
            1 => true,
            _ => return Err(Error::Corrupt("invalid tombstone flag".into())),
        };
        let vector = reader.vector(config.dimensions)?;
        config
            .validate_vector(&vector)
            .map_err(|e| Error::Corrupt(e.to_string()))?;
        let metadata = reader.metadata()?;
        records.push(Record {
            id,
            vector,
            metadata,
            deleted,
        });
    }
    reader.finish()?;
    Ok(Snapshot {
        config,
        sequence,
        records,
        checksum: expected,
    })
}

pub(crate) enum Operation<'a> {
    Put {
        id: u64,
        vector: &'a [f32],
        metadata: &'a str,
    },
    Delete {
        id: u64,
    },
}

pub(crate) fn encode_frame(sequence: u64, operation: Operation<'_>) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.push(match operation {
        Operation::Put { .. } => 1,
        Operation::Delete { .. } => 2,
    });
    u64_bytes(&mut payload, sequence);
    match operation {
        Operation::Put {
            id,
            vector,
            metadata,
        } => {
            u64_bytes(&mut payload, id);
            for value in vector {
                payload.extend_from_slice(&value.to_le_bytes());
            }
            u32_bytes(&mut payload, metadata.len() as u32);
            payload.extend_from_slice(metadata.as_bytes());
        }
        Operation::Delete { id } => {
            u64_bytes(&mut payload, id);
        }
    }
    let mut frame = Vec::with_capacity(payload.len() + 12);
    let length = (payload.len() as u32).to_le_bytes();
    frame.extend_from_slice(&length);
    u32_bytes(&mut frame, crc32(&length));
    frame.extend_from_slice(&payload);
    u32_bytes(&mut frame, crc32(&payload));
    frame
}

pub(crate) fn encode_batch(sequence: u64, operations: &[Mutation<'_>]) -> Vec<u8> {
    let mut payload = vec![3];
    u64_bytes(&mut payload, sequence);
    u32_bytes(&mut payload, operations.len() as u32);
    for operation in operations {
        match operation {
            Mutation::Put {
                id,
                vector,
                metadata,
            } => {
                payload.push(1);
                u64_bytes(&mut payload, *id);
                for value in *vector {
                    payload.extend_from_slice(&value.to_le_bytes());
                }
                u32_bytes(&mut payload, metadata.len() as u32);
                payload.extend_from_slice(metadata.as_bytes());
            }
            Mutation::Delete { id } => {
                payload.push(2);
                u64_bytes(&mut payload, *id);
            }
        }
    }
    let length = (payload.len() as u32).to_le_bytes();
    let mut frame = length.to_vec();
    u32_bytes(&mut frame, crc32(&length));
    frame.extend_from_slice(&payload);
    u32_bytes(&mut frame, crc32(&payload));
    frame
}

enum Decoded {
    Put(Record),
    Delete(u64),
}
fn decode_operation(opcode: u8, fields: &mut Reader<'_>, config: &Config) -> Result<Decoded> {
    let id = fields.u64()?;
    match opcode {
        1 => {
            let vector = fields.vector(config.dimensions)?;
            config
                .validate_vector(&vector)
                .map_err(|e| Error::Corrupt(e.to_string()))?;
            Ok(Decoded::Put(Record {
                id,
                vector,
                metadata: fields.metadata()?,
                deleted: false,
            }))
        }
        2 => Ok(Decoded::Delete(id)),
        _ => Err(Error::Corrupt("unknown WAL operation".into())),
    }
}

pub(crate) fn append_frame(file: &mut File, frame: &[u8]) -> Result<()> {
    file.seek(SeekFrom::End(0))?;
    file.write_all(frame)?;
    file.sync_all()?;
    Ok(())
}

pub(crate) fn clear_wal(file: &mut File) -> Result<()> {
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.sync_all()?;
    Ok(())
}

pub(crate) struct Recovered {
    pub config: Config,
    pub records: Vec<Record>,
    pub sequence: u64,
    pub replayed: usize,
    pub skipped: usize,
    pub truncated_bytes: u64,
    pub graph: Option<GraphState>,
    pub cache_note: Option<String>,
}

pub(crate) fn recover(path: &Path, wal: &mut File) -> Result<Recovered> {
    recover_inner(path, wal, true)
}

pub(crate) fn inspect(path: &Path, wal: &mut File) -> Result<Recovered> {
    recover_inner(path, wal, false)
}

fn recover_inner(path: &Path, wal: &mut File, repair: bool) -> Result<Recovered> {
    let Snapshot {
        config,
        sequence: snapshot_sequence,
        mut records,
        checksum,
    } = read_snapshot(path)?;
    let (graph, cache_note) =
        match graph::read(path, &config, snapshot_sequence, checksum, records.len()) {
            Ok(graph) => (Some(graph), None),
            Err(error) => (None, Some(error.to_string())),
        };
    let mut active = BTreeMap::new();
    let mut encoded_bytes = records.iter().map(Record::encoded_len).sum::<usize>();
    for (node, record) in records.iter().enumerate() {
        if !record.deleted && active.insert(record.id, node).is_some() {
            return Err(Error::Corrupt("duplicate active ID in snapshot".into()));
        }
    }
    let file_length = wal.metadata()?.len();
    wal.seek(SeekFrom::Start(0))?;
    let mut reader = BufReader::new(wal.try_clone()?);
    let mut offset = 0u64;
    let mut sequence = snapshot_sequence;
    let mut previous_wal_sequence: Option<u64> = None;
    let mut replayed = 0;
    let mut skipped = 0;
    while offset < file_length {
        if file_length - offset < 8 {
            break;
        }
        let mut length_bytes = [0; 4];
        reader.read_exact(&mut length_bytes)?;
        let mut header_checksum = [0; 4];
        reader.read_exact(&mut header_checksum)?;
        if crc32(&length_bytes) != u32::from_le_bytes(header_checksum) {
            return Err(Error::Corrupt(format!(
                "WAL length checksum mismatch at byte {offset}"
            )));
        }
        let length = u32::from_le_bytes(length_bytes) as usize;
        if !(17..=MAX_FRAME).contains(&length) {
            return Err(Error::Corrupt(format!(
                "invalid WAL frame length at byte {offset}"
            )));
        }
        let frame_length = length as u64 + 12;
        if file_length - offset < frame_length {
            break;
        }
        let mut payload = vec![0; length];
        reader.read_exact(&mut payload)?;
        let mut checksum = [0; 4];
        reader.read_exact(&mut checksum)?;
        if crc32(&payload) != u32::from_le_bytes(checksum) {
            return Err(Error::Corrupt(format!(
                "WAL checksum mismatch at byte {offset}"
            )));
        }
        let mut fields = Reader::new(&payload);
        let opcode = fields.u8()?;
        let frame_sequence = fields.u64()?;
        if frame_sequence == 0
            || previous_wal_sequence.is_some_and(|prev| prev.checked_add(1) != Some(frame_sequence))
        {
            return Err(Error::Corrupt("WAL sequence is not contiguous".into()));
        }
        let operations = if opcode == 3 {
            let count = fields.u32()? as usize;
            if !(1..=MAX_BATCH_OPERATIONS).contains(&count) || count > fields.remaining() / 9 {
                return Err(Error::Corrupt("invalid batch operation count".into()));
            }
            let mut operations = Vec::with_capacity(count);
            for _ in 0..count {
                let tag = fields.u8()?;
                operations.push(decode_operation(tag, &mut fields, &config)?);
            }
            operations
        } else {
            vec![decode_operation(opcode, &mut fields, &config)?]
        };
        fields.finish()?;
        previous_wal_sequence = Some(frame_sequence);
        if frame_sequence <= snapshot_sequence {
            skipped += 1;
        } else {
            if sequence.checked_add(1) != Some(frame_sequence) {
                return Err(Error::Corrupt("WAL gap after snapshot".into()));
            }
            for operation in operations {
                if let Decoded::Put(record) = operation {
                    encoded_bytes += record.encoded_len();
                    if records.len() == MAX_RECORDS
                        || encoded_bytes + SNAPSHOT_HEADER_BYTES + 4 > MAX_SNAPSHOT_BYTES
                    {
                        return Err(Error::Corrupt(
                            "recovered records exceed storage limits".into(),
                        ));
                    }
                    if let Some(old) = active.insert(record.id, records.len()) {
                        records[old].deleted = true;
                    }
                    records.push(record);
                } else if let Decoded::Delete(id) = operation {
                    if let Some(old) = active.remove(&id) {
                        records[old].deleted = true;
                    } else if opcode != 3 {
                        return Err(Error::Corrupt("WAL deletes an inactive ID".into()));
                    }
                }
            }
            sequence = frame_sequence;
            replayed += 1;
        }
        offset += frame_length;
    }
    drop(reader);
    let truncated_bytes = file_length - offset;
    if truncated_bytes != 0 && repair {
        wal.set_len(offset)?;
        wal.sync_all()?;
    }
    wal.seek(SeekFrom::End(0))?;
    Ok(Recovered {
        config,
        records,
        sequence,
        replayed,
        skipped,
        truncated_bytes,
        graph,
        cache_note,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crc_matches_published_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xcbf43926);
        assert_eq!(crc32(b""), 0);
    }
}
