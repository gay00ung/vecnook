use super::*;

fn encoded_error(message: &str) -> Error {
    Error::Corrupt(message.into())
}

fn write_identity(
    writer: &mut stream::Writer,
    config: &Config,
    space: &EmbeddingSpace,
) -> Result<()> {
    writer.u32(config.dimensions as u32)?;
    writer.u32(config.m as u32)?;
    writer.u32(config.ef_construction as u32)?;
    writer.u64(config.seed)?;
    writer.u8(match config.metric {
        Metric::SquaredL2 => 0,
        Metric::Cosine => 1,
        Metric::InnerProduct => 2,
    })?;
    writer.u32(space.model.len() as u32)?;
    writer.bytes(space.model.as_bytes())
}
fn read_identity(reader: &mut stream::Reader) -> Result<(Config, EmbeddingSpace)> {
    let dimensions = reader.u32()? as usize;
    let m = reader.u32()? as usize;
    let ef = reader.u32()? as usize;
    let seed = reader.u64()?;
    let metric = match reader.u8()? {
        0 => Metric::SquaredL2,
        1 => Metric::Cosine,
        2 => Metric::InnerProduct,
        _ => return Err(encoded_error("unsupported generation metric")),
    };
    let length = reader.u32()? as usize;
    if !(1..=512).contains(&length) {
        return Err(encoded_error("invalid generation model length"));
    }
    let model = String::from_utf8(reader.bytes(length)?)
        .map_err(|_| encoded_error("invalid generation model UTF-8"))?;
    let config = Config::new(dimensions)
        .with_m(m)
        .with_ef_construction(ef)
        .with_seed(seed)
        .with_metric(metric);
    let space = EmbeddingSpace::new(model, dimensions, metric);
    config
        .validate()
        .map_err(|_| encoded_error("invalid generation configuration"))?;
    space
        .validate()
        .map_err(|_| encoded_error("invalid generation identity"))?;
    Ok((config, space))
}
pub(super) fn write_store(path: &Path, config: &Config, space: &EmbeddingSpace) -> Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path.join("STORE"))?;
    let mut writer = stream::Writer::new(file, 1024, "generation.store.write");
    writer.bytes(b"VECSTOR1")?;
    write_identity(&mut writer, config, space)?;
    let (file, _) = writer.finish()?;
    file.sync_all()?;
    Ok(())
}
pub(super) fn read_store(path: &Path) -> Result<(Config, EmbeddingSpace)> {
    let mut reader = stream::Reader::new(File::open(path.join("STORE"))?, 38, 1024)?;
    if reader.array::<8>()? != *b"VECSTOR1" {
        return Err(encoded_error("unsupported generation store"));
    }
    let identity = read_identity(&mut reader)?;
    reader.finish()?;
    Ok(identity)
}
pub(super) fn current(path: &Path) -> Result<Option<(u64, u32)>> {
    let file = match File::open(path.join("CURRENT")) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut reader = stream::Reader::new(file, 24, 24)?;
    if reader.array::<8>()? != *b"VECCUR01" {
        return Err(encoded_error("unsupported generation pointer"));
    }
    let generation = reader.u64()?;
    if generation == 0 {
        return Err(encoded_error("invalid zero generation"));
    }
    let checksum = reader.u32()?;
    reader.finish()?;
    Ok(Some((generation, checksum)))
}
pub(super) fn publish_current(path: &Path, generation: u64, crc: u32) -> Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path.join("CURRENT.tmp"))?;
    let mut writer = stream::Writer::new(file, 24, "generation.pointer.write");
    writer.bytes(b"VECCUR01")?;
    writer.u64(generation)?;
    writer.u32(crc)?;
    let (file, _) = writer.finish()?;
    crate::storage_io::check("generation.pointer.sync")?;
    file.sync_all()?;
    drop(file);
    crate::storage_io::check("generation.publish")?;
    fs::rename(path.join("CURRENT.tmp"), path.join("CURRENT"))?;
    crate::storage_io::check("generation.root.sync")?;
    storage::sync_directory(path)
}
pub(super) fn write_manifest(path: &Path, manifest: &Manifest) -> Result<u32> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path.join("MANIFEST"))?;
    let mut writer = stream::Writer::new(file, MAX_MANIFEST_BYTES, "generation.manifest.write");
    writer.bytes(b"VECGEN01")?;
    writer.u64(manifest.generation)?;
    writer.u64(manifest.sequence)?;
    write_identity(&mut writer, &manifest.config, &manifest.space)?;
    writer.u32(manifest.segments.len() as u32)?;
    writer.u64(manifest.entries.len() as u64)?;
    for segment in &manifest.segments {
        writer.u64(segment.length)?;
        writer.u32(segment.crc)?;
    }
    for entry in &manifest.entries {
        writer.u64(entry.id)?;
        writer.u32(entry.segment)?;
        writer.u32(entry.offset)?;
        writer.u32(entry.metadata_len)?;
    }
    let (file, crc) = writer.finish()?;
    crate::storage_io::check("generation.manifest.sync")?;
    file.sync_all()?;
    Ok(crc)
}
pub(super) fn read_manifest(path: &Path, checksum: u32) -> Result<Manifest> {
    let mut reader =
        stream::Reader::new(File::open(path.join("MANIFEST"))?, 66, MAX_MANIFEST_BYTES)?;
    if reader.array::<8>()? != *b"VECGEN01" {
        return Err(encoded_error("unsupported generation manifest"));
    }
    let generation = reader.u64()?;
    let sequence = reader.u64()?;
    let (config, space) = read_identity(&mut reader)?;
    let segments = reader.u32()? as usize;
    let count = reader.u64()?;
    if generation == 0
        || segments > MAX_SEGMENTS
        || count > MAX_RECORDS as u64
        || (count == 0) != (segments == 0)
        || segments > reader.remaining() / 12
    {
        return Err(encoded_error("generation manifest counts exceed bounds"));
    }
    let mut infos = Vec::with_capacity(segments);
    for _ in 0..segments {
        let length = reader.u64()?;
        if !(4..=MAX_SEGMENT_BYTES as u64).contains(&length) {
            return Err(encoded_error("invalid segment length"));
        }
        infos.push(SegmentInfo {
            length,
            crc: reader.u32()?,
        });
    }
    let count = count as usize;
    if count > reader.remaining() / 20 {
        return Err(encoded_error("generation rows exceed remaining bytes"));
    }
    let mut entries = Vec::with_capacity(count);
    let mut previous = None;
    for _ in 0..count {
        let id = reader.u64()?;
        let segment = reader.u32()?;
        let offset = reader.u32()?;
        let metadata_len = reader.u32()?;
        if previous.is_some_and(|previous| id <= previous)
            || segment as usize >= segments
            || metadata_len as usize > crate::config::MAX_METADATA
        {
            return Err(encoded_error("invalid generation row index"));
        }
        let end = u64::from(offset) + 12 + 4 * config.dimensions as u64 + u64::from(metadata_len);
        if end > infos[segment as usize].length - 4 {
            return Err(encoded_error("row exceeds segment"));
        }
        previous = Some(id);
        entries.push(Entry {
            id,
            segment,
            offset,
            metadata_len,
        });
    }
    if reader.finish()? != checksum {
        return Err(encoded_error("CURRENT/manifest checksum binding mismatch"));
    }
    let mut physical = entries.clone();
    physical.sort_unstable_by_key(|entry| (entry.segment, entry.offset));
    let mut iterator = physical.iter().peekable();
    for (number, segment) in infos.iter().enumerate() {
        let mut offset = 0;
        let mut count = 0;
        while iterator
            .peek()
            .is_some_and(|entry| entry.segment as usize == number)
        {
            let entry = iterator.next().unwrap();
            if u64::from(entry.offset) != offset {
                return Err(encoded_error("overlapping or missing segment rows"));
            }
            offset += 12 + 4 * config.dimensions as u64 + u64::from(entry.metadata_len);
            count += 1;
        }
        if count == 0 || offset + 4 != segment.length {
            return Err(encoded_error("segment coverage mismatch"));
        }
    }
    Ok(Manifest {
        generation,
        sequence,
        config,
        space,
        segments: infos,
        entries,
    })
}
pub(super) fn validate_segment(file: File, manifest: &Manifest, number: usize) -> Result<()> {
    let info = manifest.segments[number];
    if file.metadata()?.len() != info.length {
        return Err(encoded_error("segment length mismatch"));
    }
    let mut reader = stream::Reader::new(file, 4, MAX_SEGMENT_BYTES)?;
    let mut rows: Vec<_> = manifest
        .entries
        .iter()
        .filter(|entry| entry.segment as usize == number)
        .collect();
    rows.sort_unstable_by_key(|entry| entry.offset);
    for row in rows {
        if reader.u64()? != row.id || reader.u32()? != row.metadata_len {
            return Err(encoded_error("segment record/index mismatch"));
        }
        let vector = reader.vector(manifest.config.dimensions)?;
        manifest
            .config
            .validate_vector(&vector)
            .map_err(|_| encoded_error("invalid original segment vector"))?;
        let metadata = reader.bytes(row.metadata_len as usize)?;
        std::str::from_utf8(&metadata)
            .map_err(|_| encoded_error("invalid original metadata UTF-8"))?;
    }
    if reader.finish()? != info.crc {
        return Err(encoded_error("segment/manifest checksum binding mismatch"));
    }
    Ok(())
}
pub(super) fn validate_source(reader: &dyn SegmentReader, info: SegmentInfo) -> Result<()> {
    if reader.len() != info.length {
        return Err(encoded_error("replacement segment length mismatch"));
    }
    let mut crc = storage::Crc32::new();
    let mut buffer = [0; 64 * 1024];
    let mut offset = 0;
    while offset < info.length - 4 {
        let count = (info.length - 4 - offset).min(buffer.len() as u64) as usize;
        reader.read_exact_at(offset, &mut buffer[..count])?;
        crc.update(&buffer[..count]);
        offset += count as u64;
    }
    let mut footer = [0; 4];
    reader.read_exact_at(offset, &mut footer)?;
    if crc.finish() != info.crc || u32::from_le_bytes(footer) != info.crc {
        return Err(encoded_error("replacement segment checksum mismatch"));
    }
    Ok(())
}
