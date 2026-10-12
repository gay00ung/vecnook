use crate::{Error, Result, config::MAX_EF};

/// Explicit execution policy. Auto may choose exact search and reports why.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum SearchStrategy {
    /// Force an exhaustive eligible-set scan.
    Exact,
    /// Force graph traversal; underfill remains visible.
    Hnsw,
    #[default]
    /// Use exact search for small sets or large K, and repair graph underfill.
    Auto,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Why the reported execution mode was selected.
#[non_exhaustive]
pub enum SearchReason {
    /// Caller selected exact search.
    ExactRequested,
    /// Caller selected graph search.
    HnswRequested,
    /// Eligible count is no larger than the exact threshold.
    SmallEligibleSet,
    /// Requested eligible Top-K exceeds the candidate pool.
    LargeK,
    /// Auto selected graph traversal.
    GraphSelected,
    /// Auto repaired graph underfill with an exact scan.
    InsufficientGraphCandidates,
}

#[derive(Clone, Copy, Debug)]
/// Per-query strategy, candidate pool and exact-scan threshold.
#[non_exhaustive]
pub struct SearchOptions {
    /// Requested algorithm. Default Auto reports its decision.
    pub strategy: SearchStrategy,
    /// Candidate pool in 1..=4096; exact strategy ignores it.
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
    /// Select the query execution policy.
    pub fn with_strategy(mut self, strategy: SearchStrategy) -> Self {
        self.strategy = strategy;
        self
    }
    /// Set the graph candidate pool.
    pub fn with_ef_search(mut self, ef: usize) -> Self {
        self.ef_search = ef;
        self
    }
    /// Set the eligible-set size at which Auto chooses exact search.
    pub fn with_exact_threshold(mut self, threshold: usize) -> Self {
        self.exact_threshold = threshold;
        self
    }
    /// Validate options before submitting work; Exact ignores the candidate pool.
    pub fn validate(&self) -> Result<()> {
        if self.strategy != SearchStrategy::Exact && !(1..=MAX_EF).contains(&self.ef_search) {
            return Err(Error::InvalidInput(format!(
                "efSearch must be 1..={MAX_EF}"
            )));
        }
        Ok(())
    }
}
