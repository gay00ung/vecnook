//! Disposable, bounded graph cache bound to an authoritative snapshot.
use crate::{
    Config, Error, Result, VectorIndex,
    config::MAX_RECORDS,
    index::GraphState,
    storage::{Reader, crc32, sync_directory, u32_bytes, u64_bytes},
};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::Path,
};

const MAGIC: &[u8; 8] = b"VECGRAPH";
const MAX_BYTES: usize = 128 * 1024 * 1024;

pub(crate) fn write(
    path: &Path,
    index: &VectorIndex,
    sequence: u64,
    snapshot_crc: u32,
) -> Result<()> {
    let graph = index.graph();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    u32_bytes(&mut bytes, 1);
    u32_bytes(&mut bytes, snapshot_crc);
    u64_bytes(&mut bytes, sequence);
    u64_bytes(&mut bytes, graph.links.len() as u64);
    u64_bytes(&mut bytes, graph.entry.map_or(u64::MAX, |n| n as u64));
    u64_bytes(&mut bytes, graph.rng_state);
    for layers in graph.links {
        bytes.push(layers.len() as u8);
        for links in layers {
            u32_bytes(&mut bytes, links.len() as u32);
            for link in links {
                u32_bytes(&mut bytes, link as u32);
            }
        }
    }
    if bytes.len() + 4 > MAX_BYTES {
        return Err(Error::InvalidInput("graph cache exceeds size limit".into()));
    }
    let crc = crc32(&bytes);
    u32_bytes(&mut bytes, crc);
    let temporary = path.join("index.tmp");
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary)?;
    crate::storage_io::write(&mut file, &bytes, "cache.write")?;
    crate::storage_io::check("cache.sync")?;
    file.sync_all()?;
    drop(file);
    crate::storage_io::check("cache.rename")?;
    fs::rename(temporary, path.join("index.bin"))?;
    sync_directory(path)
}

pub(crate) fn read(
    path: &Path,
    config: &Config,
    sequence: u64,
    snapshot_crc: u32,
    count: usize,
) -> Result<GraphState> {
    let file = File::open(path.join("index.bin"))?;
    if file.metadata()?.len() > MAX_BYTES as u64 {
        return Err(Error::Corrupt("graph cache exceeds size limit".into()));
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() < 52 || bytes.len() > MAX_BYTES {
        return Err(Error::Corrupt("invalid graph cache length".into()));
    }
    let end = bytes.len() - 4;
    if crc32(&bytes[..end]) != u32::from_le_bytes(bytes[end..].try_into().unwrap()) {
        return Err(Error::Corrupt("graph cache checksum mismatch".into()));
    }
    let mut reader = Reader::new(&bytes[..end]);
    if reader.take(8)? != MAGIC
        || reader.u32()? != 1
        || reader.u32()? != snapshot_crc
        || reader.u64()? != sequence
    {
        return Err(Error::Corrupt("stale or unsupported graph cache".into()));
    }
    let nodes = reader.u64()?;
    if nodes != count as u64 || nodes > MAX_RECORDS as u64 {
        return Err(Error::Corrupt("invalid graph node count".into()));
    }
    let entry = reader.u64()?;
    let entry = if entry == u64::MAX {
        None
    } else if entry < nodes {
        Some(entry as usize)
    } else {
        return Err(Error::Corrupt("invalid graph entry".into()));
    };
    if (count == 0) != entry.is_none() {
        return Err(Error::Corrupt("missing graph entry".into()));
    }
    let rng_state = reader.u64()?;
    if rng_state
        != config
            .seed
            .wrapping_add((count as u64).wrapping_mul(0x9e3779b97f4a7c15))
    {
        return Err(Error::Corrupt("invalid graph RNG state".into()));
    }
    if count > reader.remaining() / 5 {
        return Err(Error::Corrupt("graph counts exceed remaining bytes".into()));
    }
    let mut links: Vec<Vec<Vec<usize>>> = Vec::with_capacity(count);
    for node in 0..count {
        let levels = reader.u8()? as usize;
        if !(1..=33).contains(&levels) {
            return Err(Error::Corrupt("invalid graph levels".into()));
        }
        let mut layers = Vec::with_capacity(levels);
        for level in 0..levels {
            let degree = reader.u32()? as usize;
            if degree > config.m * if level == 0 { 2 } else { 1 } {
                return Err(Error::Corrupt("graph degree exceeds limit".into()));
            }
            let mut neighbors = Vec::with_capacity(degree);
            for _ in 0..degree {
                let neighbor = reader.u32()? as usize;
                if neighbor >= count || neighbor == node || neighbors.contains(&neighbor) {
                    return Err(Error::Corrupt("invalid graph neighbor".into()));
                }
                neighbors.push(neighbor);
            }
            layers.push(neighbors);
        }
        links.push(layers);
    }
    reader.finish()?;
    for layers in &links {
        for (level, neighbors) in layers.iter().enumerate() {
            if neighbors.iter().any(|&n| links[n].len() <= level) {
                return Err(Error::Corrupt("graph edge crosses missing layer".into()));
            }
        }
    }
    if entry.is_some_and(|entry| links.iter().any(|n| n.len() > links[entry].len())) {
        return Err(Error::Corrupt("graph entry below highest level".into()));
    }
    Ok(GraphState {
        links,
        entry,
        rng_state,
    })
}
