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
    /// Sum of squared coordinate differences (default).
    SquaredL2,
    /// One minus normalized dot product; zero vectors are rejected.
    Cosine,
    /// Negative dot product, preserving ascending distance order.
    InnerProduct,
}

impl Metric {
    /// Stable distance label used in CLI output.
    pub fn name(self) -> &'static str {
        match self {
            Self::SquaredL2 => "squared_l2",
            Self::Cosine => "cosine",
            Self::InnerProduct => "negative_inner_product",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Fixed construction settings for an index and its persisted database.
pub struct Config {
    /// Fixed coordinate count in 1..=4096.
    pub dimensions: usize,
    /// Graph neighbor bound in 2..=64; layer zero allows up to 2*M neighbors.
    pub m: usize,
    /// Insertion candidate pool in M..=4096.
    pub ef_construction: usize,
    /// Deterministic graph level seed.
    pub seed: u64,
    /// Fixed distance function.
    pub metric: Metric,
}

impl Config {
    /// Select the distance metric before creating an index or database.
    pub fn with_metric(mut self, metric: Metric) -> Self {
        self.metric = metric;
        self
    }

    /// Set dimensions with M=16, efConstruction=200, seed=42 and squared L2.
    /// The constructor is infallible; call `validate` or create an index to check limits.
    pub fn new(dimensions: usize) -> Self {
        Self {
            dimensions,
            m: 16,
            ef_construction: 200,
            seed: 42,
            metric: Metric::SquaredL2,
        }
    }

    /// Check dimensions (1..4096), M (2..64) and efConstruction (M..4096).
    /// Returns [`Error::InvalidInput`] for an out-of-range field.
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
