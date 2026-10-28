use super::*;

pub(super) const MAX_CODE_BYTES: usize = 512 * 1024 * 1024;
pub(super) const MAX_CANDIDATES: usize = 65_536;

/// Derived per-vector symmetric int8 codes with bounded candidate selection.
///
/// Original f32 coordinates stay in the pinned segment files. Exact search and
/// returned reranked distances always use those originals. Building codes reads
/// every original once; it does not reduce disk storage or guarantee recall.
pub struct QuantizedSnapshot<'a> {
    snapshot: &'a Snapshot,
    codes: Vec<i8>,
    scales: Vec<f64>,
    squared_norms: Vec<i64>,
}
impl<'a> QuantizedSnapshot<'a> {
    pub(super) fn build(snapshot: &'a Snapshot) -> Result<Self> {
        let dimensions = snapshot.space().dimensions;
        let bytes = snapshot
            .len()
            .checked_mul(dimensions)
            .ok_or_else(|| Error::InvalidInput("SQ8 size overflow".into()))?;
        if bytes > MAX_CODE_BYTES {
            return Err(Error::Capacity {
                resource: "SQ8 code bytes",
                limit: MAX_CODE_BYTES,
                required: bytes,
            });
        }
        let mut codes = Vec::with_capacity(bytes);
        let mut scales = Vec::with_capacity(snapshot.len());
        let mut squared_norms = Vec::with_capacity(snapshot.len());
        for number in 0..snapshot.len() {
            let record = snapshot.read_record(number)?;
            let (encoded, scale, norm) = encode(&record.vector);
            codes.extend_from_slice(&encoded);
            scales.push(scale);
            squared_norms.push(norm);
        }
        Ok(Self {
            snapshot,
            codes,
            scales,
            squared_norms,
        })
    }
    /// Raw int8 coordinate bytes only, excluding parameters, reader indexes and RSS.
    pub fn code_bytes(&self) -> usize {
        self.codes.len()
    }
    /// Encoded scale/norm array bytes, excluding allocator overhead.
    pub fn parameter_bytes(&self) -> usize {
        self.scales.len() * 16
    }
    /// Read the pinned generation without losing its original f32 payloads.
    pub fn snapshot(&self) -> &Snapshot {
        self.snapshot
    }
    /// Scan int8 codes, select at most candidate_limit records, then rerank using
    /// original f32 vectors. candidate_limit must be at least K and at most 65,536.
    /// Selecting every eligible row restores the exact reference ordering.
    pub fn search(
        &self,
        query: &[f32],
        k: usize,
        candidate_limit: usize,
    ) -> Result<GenerationSearchReport> {
        self.search_selected(query, k, candidate_limit, &ScalarKernel, None)
    }
    /// Search an explicit set of eligible IDs. Missing IDs are ignored and duplicate
    /// IDs are deduplicated before candidate selection; excluded IDs cannot leak.
    /// The caller can derive this set using its document/authorization layer.
    pub fn search_ids(
        &self,
        query: &[f32],
        k: usize,
        candidate_limit: usize,
        kernel: &dyn DistanceKernel,
        ids: &[u64],
    ) -> Result<GenerationSearchReport> {
        if ids.len() > MAX_RECORDS {
            return Err(Error::InvalidInput(
                "eligible ID list exceeds generation record bound".into(),
            ));
        }
        let allowed: std::collections::BTreeSet<_> = ids.iter().copied().collect();
        self.search_selected(query, k, candidate_limit, kernel, Some(&allowed))
    }
    /// Use an explicit arithmetic kernel during original-f32 reranking.
    pub fn search_with_kernel(
        &self,
        query: &[f32],
        k: usize,
        candidate_limit: usize,
        kernel: &dyn DistanceKernel,
    ) -> Result<GenerationSearchReport> {
        self.search_selected(query, k, candidate_limit, kernel, None)
    }
    fn search_selected(
        &self,
        query: &[f32],
        k: usize,
        candidate_limit: usize,
        kernel: &dyn DistanceKernel,
        ids: Option<&std::collections::BTreeSet<u64>>,
    ) -> Result<GenerationSearchReport> {
        self.snapshot.validate_query(query, k)?;
        if candidate_limit < k || candidate_limit > MAX_CANDIDATES {
            return Err(Error::InvalidInput(
                "SQ8 candidate limit must be K..=65536".into(),
            ));
        }
        if k == 0 {
            let eligible = self
                .snapshot
                .manifest
                .entries
                .iter()
                .filter(|entry| ids.is_none_or(|ids| ids.contains(&entry.id)))
                .count();
            return self
                .snapshot
                .finish_search(BinaryHeap::new(), eligible, 0, false, 0);
        }
        let (query_codes, query_scale, query_norm) = encode(query);
        let mut candidates = BinaryHeap::new();
        let mut eligible = 0;
        let dimensions = self.snapshot.space().dimensions;
        for (number, entry) in self.snapshot.manifest.entries.iter().enumerate() {
            if ids.is_some_and(|ids| !ids.contains(&entry.id)) {
                continue;
            }
            eligible += 1;
            let codes = &self.codes[number * dimensions..(number + 1) * dimensions];
            let dot: i64 = codes
                .iter()
                .zip(&query_codes)
                .map(|(&a, &b)| i64::from(a) * i64::from(b))
                .sum();
            let scale = self.scales[number];
            let norm = self.squared_norms[number];
            let distance = approximate_distance(
                self.snapshot.space().metric,
                scale,
                norm,
                query_scale,
                query_norm,
                dot,
            );
            keep_best(
                &mut candidates,
                Ranked {
                    distance,
                    id: entry.id,
                    number,
                },
                candidate_limit,
            );
        }
        rerank(
            self.snapshot,
            query,
            k,
            candidates,
            eligible,
            kernel,
            candidate_limit < eligible,
        )
    }
}
pub(super) fn approximate_distance(
    metric: Metric,
    scale: f64,
    norm: i64,
    query_scale: f64,
    query_norm: i64,
    dot: i64,
) -> f64 {
    match metric {
        Metric::SquaredL2 => (scale * scale * norm as f64
            + query_scale * query_scale * query_norm as f64
            - 2.0 * scale * query_scale * dot as f64)
            .max(0.0),
        Metric::InnerProduct => -scale * query_scale * dot as f64,
        Metric::Cosine => {
            (1.0 - dot as f64 / ((norm as f64).sqrt() * (query_norm as f64).sqrt())).clamp(0.0, 2.0)
        }
    }
}
pub(super) fn rerank(
    snapshot: &Snapshot,
    query: &[f32],
    k: usize,
    candidates: BinaryHeap<Ranked>,
    eligible: usize,
    kernel: &dyn DistanceKernel,
    approximate: bool,
) -> Result<GenerationSearchReport> {
    let work = candidates.len();
    let mut exact = BinaryHeap::new();
    for candidate in candidates {
        let record = snapshot.read_record(candidate.number)?;
        let distance = original_distance(snapshot.space().metric, query, &record.vector, kernel)?;
        keep_best(
            &mut exact,
            Ranked {
                distance,
                ..candidate
            },
            k,
        );
    }
    snapshot.finish_search(exact, eligible, work, approximate, k)
}
pub(super) fn encode(vector: &[f32]) -> (Vec<i8>, f64, i64) {
    let maximum = vector
        .iter()
        .map(|&value| f64::from(value).abs())
        .fold(0.0, f64::max);
    let scale = if maximum == 0.0 { 1.0 } else { maximum / 127.0 };
    let codes: Vec<i8> = vector
        .iter()
        .map(|&value| (f64::from(value) / scale).round().clamp(-127.0, 127.0) as i8)
        .collect();
    let norm = codes
        .iter()
        .map(|&value| i64::from(value) * i64::from(value))
        .sum();
    (codes, scale, norm)
}
