//! Disposable, bounded graph cache bound to an authoritative snapshot.
use crate::{
    Config, Error, Result, VectorIndex, config::MAX_RECORDS, index::GraphState,
    storage::sync_directory,
};
use std::{
    fs::{self, File, OpenOptions},
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
    let (entry, rng_state) = index.graph_header();
    let temporary = path.join("index.tmp");
    let file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary)?;
    let mut writer = crate::stream::Writer::new(file, MAX_BYTES, "cache.write");
    writer.bytes(MAGIC)?;
    writer.u32(1)?;
    writer.u32(snapshot_crc)?;
    writer.u64(sequence)?;
    writer.u64(index.capacity().physical_nodes as u64)?;
    writer.u64(entry.map_or(u64::MAX, |n| n as u64))?;
    writer.u64(rng_state)?;
    for layers in index.graph_links() {
        writer.u8(layers.len() as u8)?;
        for links in layers {
            writer.u32(links.len() as u32)?;
            for &link in links {
                writer.u32(link as u32)?;
            }
        }
    }
    let (file, _) = writer.finish()?;
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
    let mut reader = crate::stream::Reader::new(file, 52, MAX_BYTES)?;
    if &reader.array::<8>()? != MAGIC
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
