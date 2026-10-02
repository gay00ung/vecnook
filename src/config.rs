use crate::{Error, Result};

pub(crate) const MAX_DIMENSIONS: usize = 4096;
pub(crate) const MAX_EF: usize = 4096;
pub(crate) const MAX_RECORDS: usize = 100_000;
pub(crate) const MAX_METADATA: usize = 16 * 1024;
pub(crate) const MAX_SNAPSHOT_BYTES: usize = 256 * 1024 * 1024;
pub(crate) const SNAPSHOT_HEADER_BYTES: usize = 52;
pub(crate) const MAX_BATCH_OPERATIONS: usize = 1024;
pub(crate) const MAX_BATCH_BYTES: usize = 8 * 1024 * 1024;

/// All distances sort ascending. InnerProduct is the negative dot product.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Metric {
    #[default]
    SquaredL2,
    Cosine,
    InnerProduct,
}

impl Metric {
    pub fn name(self) -> &'static str {
        match self {
            Self::SquaredL2 => "squared_l2",
            Self::Cosine => "cosine",
            Self::InnerProduct => "negative_inner_product",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub dimensions: usize,
    pub m: usize,
    pub ef_construction: usize,
    pub seed: u64,
    pub metric: Metric,
}

impl Config {
    pub fn with_metric(mut self, metric: Metric) -> Self {
        self.metric = metric;
        self
    }

    pub fn new(dimensions: usize) -> Self {
        Self {
            dimensions,
            m: 16,
            ef_construction: 200,
            seed: 42,
            metric: Metric::SquaredL2,
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
        if self.metric == Metric::Cosine && vector.iter().all(|&x| x == 0.0) {
            return Err(Error::InvalidInput(
                "cosine requires a nonzero vector".into(),
            ));
        }
        Ok(())
    }
}
