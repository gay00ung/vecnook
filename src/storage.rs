use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{BufReader, Read, Seek, SeekFrom, Write},
    path::Path,
};

use crate::{
    Config, Error, Record, Result, VectorIndex,
    config::{
        MAX_DIMENSIONS, MAX_METADATA, MAX_RECORDS, MAX_SNAPSHOT_BYTES, SNAPSHOT_HEADER_BYTES,
    },
};

const MAGIC: &[u8; 8] = b"VECTORS1";
const VERSION: u32 = 1;
const MAX_FRAME: usize = 17 + MAX_DIMENSIONS * 4 + 4 + MAX_METADATA;

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

fn u32_bytes(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn u64_bytes(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }
    fn take(&mut self, length: usize) -> Result<&'a [u8]> {
        if length > self.remaining() {
            return Err(Error::Corrupt("truncated field".into()));
        }
        let start = self.offset;
        self.offset += length;
        Ok(&self.bytes[start..self.offset])
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn vector(&mut self, dimensions: usize) -> Result<Vec<f32>> {
        let raw = self.take(dimensions * 4)?;
        let values: Vec<_> = raw
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
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
    fn finish(&self) -> Result<()> {
        if self.remaining() == 0 {
            Ok(())
        } else {
            Err(Error::Corrupt("unexpected trailing fields".into()))
        }
    }
}

pub(crate) fn ensure_platform() -> Result<()> {
    if cfg!(any(target_os = "macos", target_os = "linux")) {
        Ok(())
    } else {
        Err(Error::UnsupportedPlatform)
    }
}

pub(crate) fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
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
    sync_directory(path)
}

fn read_snapshot(path: &Path) -> Result<(Config, u64, Vec<Record>)> {
    let file = File::open(path.join("snapshot.bin"))?;
    if file.metadata()?.len() > MAX_SNAPSHOT_BYTES as u64 {
        return Err(Error::Corrupt("snapshot exceeds size limit".into()));
    }
    let mut bytes = Vec::new();
    file.take(MAX_SNAPSHOT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() < SNAPSHOT_HEADER_BYTES + 4 || bytes.len() > MAX_SNAPSHOT_BYTES {
        return Err(Error::Corrupt("invalid snapshot length".into()));
    }
    let checksum_offset = bytes.len() - 4;
    let expected = u32::from_le_bytes(bytes[checksum_offset..].try_into().unwrap());
    if crc32(&bytes[..checksum_offset]) != expected {
        return Err(Error::Corrupt("snapshot checksum mismatch".into()));
    }
    let mut reader = Reader::new(&bytes[..checksum_offset]);
    if reader.take(8)? != MAGIC || reader.u32()? != VERSION {
        return Err(Error::Corrupt("unknown snapshot magic/version".into()));
    }
    let config = Config {
        dimensions: reader.u32()? as usize,
        m: reader.u32()? as usize,
        ef_construction: reader.u32()? as usize,
        seed: reader.u64()?,
    };
    config
        .validate()
        .map_err(|e| Error::Corrupt(e.to_string()))?;
    let sequence = reader.u64()?;
    let count = reader.u64()?;
    let minimum_record = 13 + config.dimensions * 4;
    if count > MAX_RECORDS as u64
        || count > (reader.remaining() / minimum_record) as u64
        || count > sequence
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
        let metadata = reader.metadata()?;
        records.push(Record {
            id,
            vector,
            metadata,
            deleted,
        });
    }
    reader.finish()?;
    Ok((config, sequence, records))
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
}

pub(crate) fn recover(path: &Path, wal: &mut File) -> Result<Recovered> {
    let (config, snapshot_sequence, mut records) = read_snapshot(path)?;
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
        let id = fields.u64()?;
        if frame_sequence == 0
            || previous_wal_sequence.is_some_and(|prev| prev.checked_add(1) != Some(frame_sequence))
        {
            return Err(Error::Corrupt("WAL sequence is not contiguous".into()));
        }
        let put = match opcode {
            1 => Some(Record {
                id,
                vector: fields.vector(config.dimensions)?,
                metadata: fields.metadata()?,
                deleted: false,
            }),
            2 => None,
            _ => return Err(Error::Corrupt("unknown WAL operation".into())),
        };
        fields.finish()?;
        previous_wal_sequence = Some(frame_sequence);
        if frame_sequence <= snapshot_sequence {
            skipped += 1;
        } else {
            if sequence.checked_add(1) != Some(frame_sequence) {
                return Err(Error::Corrupt("WAL gap after snapshot".into()));
            }
            if let Some(record) = put {
                encoded_bytes += record.encoded_len();
                if records.len() == MAX_RECORDS
                    || encoded_bytes + SNAPSHOT_HEADER_BYTES + 4 > MAX_SNAPSHOT_BYTES
                {
                    return Err(Error::Corrupt(
                        "recovered records exceed storage limits".into(),
                    ));
                }
                if let Some(old) = active.insert(id, records.len()) {
                    records[old].deleted = true;
                }
                records.push(record);
            } else if let Some(old) = active.remove(&id) {
                records[old].deleted = true;
            } else {
                return Err(Error::Corrupt("WAL deletes an inactive ID".into()));
            }
            sequence = frame_sequence;
            replayed += 1;
        }
        offset += frame_length;
    }
    drop(reader);
    let truncated_bytes = file_length - offset;
    if truncated_bytes != 0 {
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
