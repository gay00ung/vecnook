//! Bounded, checksummed persistence without retaining a second encoded file in memory.
use crate::{Error, Result, storage::Crc32};
use std::{
    fs::File,
    io::{BufReader, Read},
};

const BUFFER: usize = 64 * 1024;

pub(crate) struct Writer {
    file: File,
    buffer: Vec<u8>,
    crc: Crc32,
    written: usize,
    limit: usize,
    point: &'static str,
}
impl Writer {
    pub(crate) fn new(file: File, limit: usize, point: &'static str) -> Self {
        Self {
            file,
            buffer: Vec::with_capacity(BUFFER),
            crc: Crc32::new(),
            written: 0,
            limit,
            point,
        }
    }
    pub(crate) fn bytes(&mut self, mut bytes: &[u8]) -> Result<()> {
        if bytes.len() > self.limit.saturating_sub(self.written + 4) {
            return Err(Error::InvalidInput(
                "encoded file exceeds size limit".into(),
            ));
        }
        self.written += bytes.len();
        self.crc.update(bytes);
        while !bytes.is_empty() {
            let count = bytes.len().min(BUFFER - self.buffer.len());
            self.buffer.extend_from_slice(&bytes[..count]);
            bytes = &bytes[count..];
            if self.buffer.len() == BUFFER {
                self.flush()?;
            }
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<()> {
        if !self.buffer.is_empty() {
            crate::storage_io::write(&mut self.file, &self.buffer, self.point)?;
            self.buffer.clear();
        }
        Ok(())
    }
    pub(crate) fn u8(&mut self, value: u8) -> Result<()> {
        self.bytes(&[value])
    }
    pub(crate) fn u32(&mut self, value: u32) -> Result<()> {
        self.bytes(&value.to_le_bytes())
    }
    pub(crate) fn u64(&mut self, value: u64) -> Result<()> {
        self.bytes(&value.to_le_bytes())
    }
    pub(crate) fn finish(mut self) -> Result<(File, u32)> {
        self.flush()?;
        let checksum = self.crc.finish();
        crate::storage_io::write(&mut self.file, &checksum.to_le_bytes(), self.point)?;
        Ok((self.file, checksum))
    }
}

pub(crate) struct Reader {
    file: BufReader<File>,
    remaining: usize,
    crc: Crc32,
}
impl Reader {
    pub(crate) fn new(file: File, minimum: usize, maximum: usize) -> Result<Self> {
        let length = file.metadata()?.len();
        if length < minimum as u64 || length > maximum as u64 {
            return Err(Error::Corrupt("invalid encoded file length".into()));
        }
        Ok(Self {
            file: BufReader::with_capacity(BUFFER, file),
            remaining: length as usize - 4,
            crc: Crc32::new(),
        })
    }
    pub(crate) fn remaining(&self) -> usize {
        self.remaining
    }
    fn read(&mut self, target: &mut [u8]) -> Result<()> {
        if target.len() > self.remaining {
            return Err(Error::Corrupt("truncated field".into()));
        }
        self.file.read_exact(target).map_err(read_error)?;
        self.remaining -= target.len();
        self.crc.update(target);
        Ok(())
    }
    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut result = [0; N];
        self.read(&mut result)?;
        Ok(result)
    }
    pub(crate) fn bytes(&mut self, length: usize) -> Result<Vec<u8>> {
        if length > self.remaining {
            return Err(Error::Corrupt("truncated field".into()));
        }
        let mut bytes = vec![0; length];
        self.read(&mut bytes)?;
        Ok(bytes)
    }
    pub(crate) fn u8(&mut self) -> Result<u8> {
        Ok(self.array::<1>()?[0])
    }
    pub(crate) fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }
    pub(crate) fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.array()?))
    }
    pub(crate) fn vector(&mut self, dimensions: usize) -> Result<Vec<f32>> {
        if dimensions > self.remaining / 4 {
            return Err(Error::Corrupt("truncated vector".into()));
        }
        let mut vector = Vec::with_capacity(dimensions);
        for _ in 0..dimensions {
            let value = f32::from_le_bytes(self.array()?);
            if !value.is_finite() {
                return Err(Error::Corrupt("non-finite coordinate".into()));
            }
            vector.push(value);
        }
        Ok(vector)
    }
    pub(crate) fn finish(mut self) -> Result<u32> {
        if self.remaining != 0 {
            return Err(Error::Corrupt("unexpected trailing fields".into()));
        }
        let mut checksum = [0; 4];
        self.file.read_exact(&mut checksum).map_err(read_error)?;
        let expected = u32::from_le_bytes(checksum);
        if self.crc.finish() != expected {
            return Err(Error::Corrupt("file checksum mismatch".into()));
        }
        if self.file.read(&mut [0])? != 0 {
            return Err(Error::Corrupt("unexpected trailing bytes".into()));
        }
        Ok(expected)
    }
}
fn read_error(error: std::io::Error) -> Error {
    if error.kind() == std::io::ErrorKind::UnexpectedEof {
        Error::Corrupt("truncated file".into())
    } else {
        Error::Io(error)
    }
}
