use crate::{Error, Result, config::MAX_EF};

/// Explicit execution policy. Auto may choose exact search and reports why.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchStrategy {
    Exact,
    Hnsw,
    #[default]
    Auto,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchReason {
    ExactRequested,
    HnswRequested,
    SmallEligibleSet,
    LargeK,
    GraphSelected,
    InsufficientGraphCandidates,
}

#[derive(Clone, Copy, Debug)]
pub struct SearchOptions {
    pub strategy: SearchStrategy,
    pub ef_search: usize,
    /// Auto scans the eligible set exactly when it is no larger than this.
    pub exact_threshold: usize,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            strategy: SearchStrategy::Auto,
            ef_search: 128,
            exact_threshold: 256,
        }
    }
}

impl SearchOptions {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.strategy != SearchStrategy::Exact && !(1..=MAX_EF).contains(&self.ef_search) {
            return Err(Error::InvalidInput(format!(
                "efSearch must be 1..={MAX_EF}"
            )));
        }
        Ok(())
    }
}
