use crate::{Error, Result, config::MAX_METADATA};
use std::collections::BTreeSet;

/// A document chunk with its original text, source and application tags.
/// The encoded payload, including all fields, must fit 16 KiB.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Document {
    /// Application ID; all u64 values are supported.
    pub id: u64,
    /// Original UTF-8 chunk text, at most 6000 bytes.
    pub text: String,
    /// Nonempty source identifier/path, at most 1024 UTF-8 bytes; no NUL.
    pub source: String,
    /// First original source line, starting at one.
    pub start_line: u32,
    /// Last original source line, inclusive and at least start_line.
    pub end_line: u32,
    /// At most 32 unique, nonempty tags of at most 128 UTF-8 bytes each.
    pub tags: Vec<String>,
}

impl Document {
    /// Create a chunk with line range 1..=1 and no tags; validated on insertion/encoding.
    pub fn new(id: u64, text: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            id,
            text: text.into(),
            source: source.into(),
            start_line: 1,
            end_line: 1,
            tags: Vec::new(),
        }
    }

    /// Encode a versioned payload for CLI batch import. The representation is
    /// hexadecimal, so tabs/newlines in original text cannot break TSV records.
    pub fn to_payload(&self) -> Result<String> {
        self.validate()?;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&self.id.to_le_bytes());
        bytes.extend_from_slice(&self.start_line.to_le_bytes());
        bytes.extend_from_slice(&self.end_line.to_le_bytes());
        put_string(&mut bytes, &self.source);
        put_string(&mut bytes, &self.text);
        bytes.extend_from_slice(&(self.tags.len() as u32).to_le_bytes());
        for tag in &self.tags {
            put_string(&mut bytes, tag);
        }
        if 6 + bytes.len() * 2 > MAX_METADATA {
            return Err(Error::InvalidInput(
                "encoded document exceeds 16 KiB".into(),
            ));
        }
        const HEX: &[u8] = b"0123456789abcdef";
        let mut payload = String::with_capacity(6 + bytes.len() * 2);
        payload.push_str("VDOC1:");
        for byte in bytes {
            payload.push(HEX[(byte >> 4) as usize] as char);
            payload.push(HEX[(byte & 15) as usize] as char);
        }
        Ok(payload)
    }

    /// Decode a bounded versioned payload; malformed or invalid data returns Corrupt.
    pub fn from_payload(payload: &str) -> Result<Self> {
        if payload.len() > MAX_METADATA {
            return Err(corrupt("document payload too large"));
        }
        let hex = payload
            .strip_prefix("VDOC1:")
            .ok_or_else(|| corrupt("document payload version"))?;
        if !hex.len().is_multiple_of(2) {
            return Err(corrupt("document hex length"));
        }
        let nibble = |c: u8| match c {
            b'0'..=b'9' => Ok(c - b'0'),
            b'a'..=b'f' => Ok(c - b'a' + 10),
            _ => Err(corrupt("document hex digit")),
        };
        let bytes: Vec<_> = hex
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| Ok((nibble(pair[0])? << 4) | nibble(pair[1])?))
            .collect::<Result<_>>()?;
        let mut reader = PayloadReader::new(&bytes);
        let id = u64::from_le_bytes(reader.take(8)?.try_into().unwrap());
        let start_line = reader.u32()?;
        let end_line = reader.u32()?;
        let source = reader.string(1024)?;
        let text = reader.string(6000)?;
        let count = reader.u32()? as usize;
        if count > 32 {
            return Err(corrupt("document tag count"));
        }
        let tags = (0..count)
            .map(|_| reader.string(128))
            .collect::<Result<_>>()?;
        reader.finish()?;
        let document = Self {
            id,
            source,
            text,
            start_line,
            end_line,
            tags,
        };
        document.validate().map_err(|e| corrupt(&e.to_string()))?;
        Ok(document)
    }

    fn validate(&self) -> Result<()> {
        if self.text.len() > 6000
            || self.source.is_empty()
            || self.source.len() > 1024
            || self.source.contains('\0')
        {
            return Err(Error::InvalidInput(
                "document text/source limits exceeded".into(),
            ));
        }
        if self.start_line == 0 || self.end_line < self.start_line {
            return Err(Error::InvalidInput("invalid document line range".into()));
        }
        let unique: BTreeSet<_> = self.tags.iter().collect();
        if self.tags.len() > 32
            || unique.len() != self.tags.len()
            || self
                .tags
                .iter()
                .any(|s| s.is_empty() || s.len() > 128 || s.contains('\0'))
        {
            return Err(Error::InvalidInput(
                "tags must be unique, nonempty and within size limits".into(),
            ));
        }
        Ok(())
    }
}

pub(crate) fn put_string(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
    bytes.extend_from_slice(value.as_bytes());
}

pub(crate) fn corrupt(message: &str) -> Error {
    Error::Corrupt(message.into())
}

pub(crate) struct PayloadReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> PayloadReader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    pub(crate) fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or_else(|| corrupt("payload length overflow"))?;
        let slice = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| corrupt("truncated payload"))?;
        self.offset = end;
        Ok(slice)
    }
    pub(crate) fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub(crate) fn string(&mut self, limit: usize) -> Result<String> {
        let count = self.u32()? as usize;
        if count > limit {
            return Err(corrupt("payload string limit"));
        }
        String::from_utf8(self.take(count)?.to_vec()).map_err(|_| corrupt("payload UTF-8"))
    }
    pub(crate) fn finish(&self) -> Result<()> {
        if self.offset != self.bytes.len() {
            Err(corrupt("trailing payload bytes"))
        } else {
            Ok(())
        }
    }
}
