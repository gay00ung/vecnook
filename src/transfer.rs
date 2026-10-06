use crate::{
    Config, Document, EmbeddingSpace, Error, Metric, Record, Result, VectorIndex,
    config::{MAX_METADATA, MAX_RECORDS, MAX_SNAPSHOT_BYTES, SNAPSHOT_HEADER_BYTES},
    storage::{self, Reader, u32_bytes, u64_bytes},
};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

const MAX_EXPORT: usize = MAX_SNAPSHOT_BYTES + 1024;
const MAGIC: &[u8; 8] = b"VECXPORT";
pub(crate) struct ImportData {
    pub config: Config,
    pub records: Vec<Record>,
    pub space: Option<EmbeddingSpace>,
}
fn string(out: &mut Vec<u8>, value: &str) {
    u32_bytes(out, value.len() as u32);
    out.extend_from_slice(value.as_bytes());
}
fn read_string(reader: &mut Reader<'_>, limit: usize) -> Result<String> {
    let length = reader.u32()? as usize;
    if length > limit {
        return Err(Error::Corrupt("export string length exceeds limit".into()));
    }
    String::from_utf8(reader.take(length)?.to_vec())
        .map_err(|_| Error::Corrupt("export UTF-8".into()))
}
pub(crate) fn write(
    path: &Path,
    index: &VectorIndex,
    space: Option<&EmbeddingSpace>,
) -> Result<()> {
    let config = index.config();
    let mut bytes = MAGIC.to_vec();
    u32_bytes(&mut bytes, 1);
    bytes.push(u8::from(space.is_some()));
    u32_bytes(&mut bytes, config.dimensions as u32);
    u32_bytes(&mut bytes, config.m as u32);
    u32_bytes(&mut bytes, config.ef_construction as u32);
    u64_bytes(&mut bytes, config.seed);
    bytes.push(match config.metric {
        Metric::SquaredL2 => 0,
        Metric::Cosine => 1,
        Metric::InnerProduct => 2,
    });
    string(&mut bytes, space.map_or("", |s| s.model.as_str()));
    u32_bytes(&mut bytes, index.len() as u32);
    for record in index.iter() {
        u64_bytes(&mut bytes, record.id);
        for v in &record.vector {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        string(&mut bytes, &record.metadata);
    }
    if bytes.len() + 4 > MAX_EXPORT {
        return Err(Error::Capacity {
            resource: "export_bytes",
            limit: MAX_EXPORT,
            required: bytes.len() + 4,
        });
    }
    let crc = storage::crc32(&bytes);
    u32_bytes(&mut bytes, crc);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Error::AlreadyExists
            } else {
                e.into()
            }
        })?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    storage::sync_directory(
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )
}
pub(crate) fn read(path: &Path) -> Result<ImportData> {
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_EXPORT as u64 {
        return Err(Error::Corrupt("export exceeds byte limit".into()));
    }
    let mut bytes = Vec::new();
    file.take(MAX_EXPORT as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() < 46 || bytes.len() > MAX_EXPORT {
        return Err(Error::Corrupt("export size".into()));
    }
    let end = bytes.len() - 4;
    if storage::crc32(&bytes[..end]) != u32::from_le_bytes(bytes[end..].try_into().unwrap()) {
        return Err(Error::Corrupt("export checksum".into()));
    }
    let mut reader = Reader::new(&bytes[..end]);
    if reader.take(8)? != MAGIC || reader.u32()? != 1 {
        return Err(Error::Corrupt("export magic/version".into()));
    }
    let kind = reader.u8()?;
    if kind > 1 {
        return Err(Error::Corrupt("export kind".into()));
    }
    let mut config = Config::new(reader.u32()? as usize)
        .with_m(reader.u32()? as usize)
        .with_ef_construction(reader.u32()? as usize)
        .with_seed(reader.u64()?);
    config.metric = match reader.u8()? {
        0 => Metric::SquaredL2,
        1 => Metric::Cosine,
        2 => Metric::InnerProduct,
        _ => return Err(Error::Corrupt("export metric".into())),
    };
    config
        .validate()
        .map_err(|e| Error::Corrupt(e.to_string()))?;
    let model = read_string(&mut reader, 512)?;
    let space = if kind == 1 {
        let s = EmbeddingSpace::new(model, config.dimensions, config.metric);
        s.validate().map_err(|e| Error::Corrupt(e.to_string()))?;
        Some(s)
    } else {
        if !model.is_empty() {
            return Err(Error::Corrupt("vector export carries model".into()));
        }
        None
    };
    let count = reader.u32()? as usize;
    if count > MAX_RECORDS || count > reader.remaining() / (12 + 4 * config.dimensions) {
        return Err(Error::Corrupt("export count".into()));
    }
    let mut records = Vec::with_capacity(count);
    let mut ids = BTreeSet::new();
    let mut encoded = SNAPSHOT_HEADER_BYTES + 4;
    for _ in 0..count {
        let id = reader.u64()?;
        if !ids.insert(id) {
            return Err(Error::Corrupt("duplicate export ID".into()));
        }
        let vector = reader.vector(config.dimensions)?;
        config
            .validate_vector(&vector)
            .map_err(|e| Error::Corrupt(e.to_string()))?;
        let metadata = read_string(&mut reader, MAX_METADATA)?;
        if space.is_some() && Document::from_payload(&metadata)?.id != id {
            return Err(Error::Corrupt("export document ID differs".into()));
        }
        let record = Record {
            id,
            vector,
            metadata,
            deleted: false,
        };
        encoded += record.encoded_len();
        if encoded > MAX_SNAPSHOT_BYTES {
            return Err(Error::Corrupt("import exceeds snapshot limit".into()));
        }
        records.push(record);
    }
    reader.finish()?;
    Ok(ImportData {
        config,
        records,
        space,
    })
}
pub(crate) fn claim_directory(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            Error::AlreadyExists
        } else {
            e.into()
        }
    })
}
