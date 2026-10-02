use crate::{Error, Result};

pub(crate) const MAX_DIMENSIONS: usize = 4096;
pub(crate) const MAX_EF: usize = 4096;
pub(crate) const MAX_RECORDS: usize = 100_000;
pub(crate) const MAX_METADATA: usize = 16 * 1024;
pub(crate) const MAX_SNAPSHOT_BYTES: usize = 256 * 1024 * 1024;
pub(crate) const SNAPSHOT_HEADER_BYTES: usize = 48;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub dimensions: usize,
    pub m: usize,
    pub ef_construction: usize,
    pub seed: u64,
}

impl Config {
    pub fn new(dimensions: usize) -> Self {
        Self {
            dimensions,
            m: 16,
            ef_construction: 200,
            seed: 42,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if !(1..=MAX_DIMENSIONS).contains(&self.dimensions) {
            return Err(Error::InvalidInput(format!(
                "dimensions must be 1..={MAX_DIMENSIONS}"
            )));
        }
        if !(2..=64).contains(&self.m) {
            return Err(Error::InvalidInput("M must be 2..=64".into()));
        }
        if !(self.m..=MAX_EF).contains(&self.ef_construction) {
            return Err(Error::InvalidInput(format!(
                "efConstruction must be M..={MAX_EF}"
            )));
        }
        Ok(())
    }

    pub(crate) fn validate_vector(&self, vector: &[f32]) -> Result<()> {
        if vector.len() != self.dimensions {
            return Err(Error::InvalidInput(format!(
                "expected {} coordinates, received {}",
                self.dimensions,
                vector.len()
            )));
        }
        if vector.iter().any(|x| !x.is_finite()) {
            return Err(Error::InvalidInput(
                "coordinates must be finite (no NaN/infinity)".into(),
            ));
        }
        Ok(())
    }
}
