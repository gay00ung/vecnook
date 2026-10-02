use std::{
    cmp::{Ordering, Reverse},
    collections::{BTreeMap, BinaryHeap},
};

use crate::{
    BatchReport, Config, Error, Metric, Mutation, Result, SearchOptions, SearchReason,
    SearchStrategy,
    config::{
        MAX_BATCH_BYTES, MAX_BATCH_OPERATIONS, MAX_EF, MAX_METADATA, MAX_RECORDS,
        MAX_SNAPSHOT_BYTES, SNAPSHOT_HEADER_BYTES,
    },
    math::{self, Query},
    rng::Rng,
};

#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub id: u64,
    pub vector: Vec<f32>,
    pub metadata: String,
    pub(crate) deleted: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_reports_exact_repair_but_forced_hnsw_keeps_underfill_visible() {
        let mut index = VectorIndex::new(Config::new(1)).unwrap();
        for id in 0..40 {
            index.put(id, &[id as f32], "").unwrap();
        }
        // A structurally valid but disconnected graph simulates candidate loss.
        for node in &mut index.nodes {
            for layer in &mut node.links {
                layer.clear();
            }
        }
        index.check_invariants().unwrap();
        let forced = index
            .search(
                &[10.0],
                5,
                SearchOptions {
                    strategy: SearchStrategy::Hnsw,
                    exact_threshold: 0,
                    ..SearchOptions::default()
                },
            )
            .unwrap();
        assert_eq!(forced.mode, SearchMode::Hnsw);
        assert!(!forced.complete);
        assert_eq!(forced.neighbors.len(), 1);
        let auto = index
            .search(
                &[10.0],
                5,
                SearchOptions {
                    exact_threshold: 0,
                    ..SearchOptions::default()
                },
            )
            .unwrap();
        assert_eq!(auto.mode, SearchMode::Exact);
        assert_eq!(auto.reason, SearchReason::InsufficientGraphCandidates);
        assert!(auto.complete);
        assert_eq!(
            auto.neighbors,
            index.search_exact(&[10.0], 5).unwrap().neighbors
        );
        assert!(auto.distance_computations > index.len());
    }
}

impl Record {
    pub(crate) fn encoded_len(&self) -> usize {
        13 + 4 * self.vector.len() + self.metadata.len()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Neighbor {
    pub id: u64,
    pub distance: f64,
    pub metadata: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchMode {
    Exact,
    Hnsw,
}

#[derive(Debug)]
pub struct SearchReport {
    pub mode: SearchMode,
    pub neighbors: Vec<Neighbor>,
    pub distance_computations: usize,
    /// True when min(K, eligible_count) candidates were found; this is not recall.
    pub complete: bool,
    pub metric: Metric,
    pub eligible_count: usize,
    pub reason: SearchReason,
}

#[derive(Clone, Debug)]
pub struct IndexStats {
    pub active_records: usize,
    pub physical_nodes: usize,
    pub tombstones: usize,
    pub layers: usize,
    pub directed_edges: usize,
    /// Raw coordinate payload only; this is not total process RSS.
    pub vector_bytes: usize,
}

struct Node {
    record: Record,
    links: Vec<Vec<usize>>,
    inverse_norm: f64,
}

pub(crate) struct GraphState {
    pub links: Vec<Vec<Vec<usize>>>,
    pub entry: Option<usize>,
    pub rng_state: u64,
}

enum Eligibility<'a> {
    All,
    Active,
    Allowed(&'a [bool]),
}

#[derive(Clone, Copy, Debug)]
struct Scored {
    distance: f64,
    id: u64,
    node: usize,
}

impl PartialEq for Scored {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Scored {}
impl PartialOrd for Scored {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Scored {
    fn cmp(&self, other: &Self) -> Ordering {
        self.distance
            .total_cmp(&other.distance)
            .then(self.id.cmp(&other.id))
            .then(self.node.cmp(&other.node))
    }
}

pub struct VectorIndex {
    config: Config,
    nodes: Vec<Node>,
    active: BTreeMap<u64, usize>,
    entry: Option<usize>,
    rng: Rng,
    encoded_bytes: usize,
}

impl VectorIndex {
    pub fn new(config: Config) -> Result<Self> {
        config.validate()?;
        let rng = Rng::new(config.seed);
        Ok(Self {
            config,
            nodes: Vec::new(),
            active: BTreeMap::new(),
            entry: None,
            rng,
            encoded_bytes: 0,
        })
    }

    pub fn config(&self) -> &Config {
        &self.config
    }
    pub fn len(&self) -> usize {
        self.active.len()
    }
    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
    }
    pub fn get(&self, id: u64) -> Option<&Record> {
        self.active.get(&id).map(|&n| &self.nodes[n].record)
    }

    pub(crate) fn validate_put(&self, vector: &[f32], metadata: &str) -> Result<()> {
        self.config.validate_vector(vector)?;
        if metadata.len() > MAX_METADATA {
            return Err(Error::InvalidInput(format!(
                "metadata exceeds {MAX_METADATA} UTF-8 bytes"
            )));
        }
        if self.nodes.len() >= MAX_RECORDS {
            return Err(Error::InvalidInput(format!(
                "physical node limit {MAX_RECORDS} reached; compact first"
            )));
        }
        let additional = 13 + 4 * vector.len() + metadata.len();
        if self.encoded_bytes + additional + SNAPSHOT_HEADER_BYTES + 4 > MAX_SNAPSHOT_BYTES {
            return Err(Error::InvalidInput(
                "snapshot size limit reached; compact first".into(),
            ));
        }
        Ok(())
    }

    /// Insert or replace an ID. Returns true for a new active ID.
    pub fn put(&mut self, id: u64, vector: &[f32], metadata: &str) -> Result<bool> {
        self.validate_put(vector, metadata)?;
        let previous = self.active.get(&id).copied();
        let node = self.append_node(Record {
            id,
            vector: vector.to_vec(),
            metadata: metadata.to_owned(),
            deleted: false,
        });
        if let Some(old) = previous {
            self.nodes[old].record.deleted = true;
        }
        self.active.insert(id, node);
        Ok(previous.is_none())
    }

    pub(crate) fn validate_batch(&self, operations: &[Mutation<'_>]) -> Result<BatchReport> {
        if operations.len() > MAX_BATCH_OPERATIONS {
            return Err(Error::InvalidInput("batch exceeds 1024 operations".into()));
        }
        let mut report = BatchReport::default();
        let mut states = BTreeMap::new();
        let mut bytes = self.encoded_bytes;
        let mut count = self.nodes.len();
        let mut payload = 13;
        for operation in operations {
            let id = match operation {
                Mutation::Put { id, .. } | Mutation::Delete { id } => *id,
            };
            let present = *states.entry(id).or_insert_with(|| self.get(id).is_some());
            match operation {
                Mutation::Put {
                    vector, metadata, ..
                } => {
                    self.config.validate_vector(vector)?;
                    if metadata.len() > MAX_METADATA {
                        return Err(Error::InvalidInput("metadata exceeds 16 KiB".into()));
                    }
                    let length = 13 + vector.len() * 4 + metadata.len();
                    bytes += length;
                    payload += length;
                    count += 1;
                    if present {
                        report.updated += 1;
                    } else {
                        report.inserted += 1;
                    }
                    states.insert(id, true);
                }
                Mutation::Delete { .. } => {
                    payload += 9;
                    if present {
                        report.deleted += 1;
                    } else {
                        report.absent += 1;
                    }
                    states.insert(id, false);
                }
            }
        }
        if count > MAX_RECORDS || bytes + SNAPSHOT_HEADER_BYTES + 4 > MAX_SNAPSHOT_BYTES {
            return Err(Error::InvalidInput(
                "batch exceeds storage limits; compact first".into(),
            ));
        }
        if payload > MAX_BATCH_BYTES {
            return Err(Error::InvalidInput(
                "batch exceeds 8 MiB WAL payload".into(),
            ));
        }
        Ok(report)
    }

    /// A missing ID is a no-op; graph links remain available for traversal.
    pub fn delete(&mut self, id: u64) -> bool {
        if let Some(node) = self.active.remove(&id) {
            self.nodes[node].record.deleted = true;
            true
        } else {
            false
        }
    }

    pub fn search_exact(&self, query: &[f32], k: usize) -> Result<SearchReport> {
        self.config.validate_vector(query)?;
        let query = Query::new(query, self.config.metric);
        let target = k.min(self.len());
        let mut best = BinaryHeap::new();
        let mut computations = 0;
        if target != 0 {
            for &node in self.active.values() {
                let candidate = self.score(&query, node, &mut computations);
                if best.len() < target || candidate < *best.peek().unwrap() {
                    best.push(candidate);
                    if best.len() > target {
                        best.pop();
                    }
                }
            }
        }
        Ok(self.report(best.into_vec(), target, computations, SearchMode::Exact))
    }

    pub fn search_hnsw(&self, query: &[f32], k: usize, ef: usize) -> Result<SearchReport> {
        self.config.validate_vector(query)?;
        let query = Query::new(query, self.config.metric);
        let target = k.min(self.len());
        if !(1..=MAX_EF).contains(&ef) || ef < target {
            return Err(Error::InvalidInput(format!(
                "efSearch must be 1..={MAX_EF} and >= min(K, active_count)"
            )));
        }
        let mut computations = 0;
        if target == 0 {
            return Ok(self.report(Vec::new(), target, 0, SearchMode::Hnsw));
        }
        let mut entry = self.entry.expect("nonempty index has an entry point");
        let top = self.nodes[entry].links.len() - 1;
        for layer in (1..=top).rev() {
            entry = self.greedy(&query, entry, layer, &mut computations);
        }
        let best = self.search_layer(&query, entry, ef, 0, Eligibility::Active, &mut computations);
        Ok(self.report(best, target, computations, SearchMode::Hnsw))
    }

    pub fn search(&self, query: &[f32], k: usize, options: SearchOptions) -> Result<SearchReport> {
        self.search_filtered(query, k, options, |_| true)
    }

    /// Evaluate the predicate once per active record, before vector search.
    /// Ineligible nodes can still serve as traversal paths. No metadata index
    /// is maintained; evaluating predicates costs O(active records).
    pub fn search_filtered<F>(
        &self,
        query: &[f32],
        k: usize,
        options: SearchOptions,
        mut filter: F,
    ) -> Result<SearchReport>
    where
        F: FnMut(&Record) -> bool,
    {
        self.config.validate_vector(query)?;
        options.validate()?;
        let query = Query::new(query, self.config.metric);
        let mut allowed = vec![false; self.nodes.len()];
        let eligible: Vec<usize> = self
            .active
            .values()
            .copied()
            .filter(|&n| {
                allowed[n] = filter(&self.nodes[n].record);
                allowed[n]
            })
            .collect();
        let target = k.min(eligible.len());
        if options.strategy == SearchStrategy::Hnsw && options.ef_search < target {
            return Err(Error::InvalidInput(
                "efSearch is smaller than the eligible Top-K".into(),
            ));
        }
        let reason = match options.strategy {
            SearchStrategy::Exact => Some(SearchReason::ExactRequested),
            SearchStrategy::Hnsw => None,
            SearchStrategy::Auto if eligible.len() <= options.exact_threshold => {
                Some(SearchReason::SmallEligibleSet)
            }
            SearchStrategy::Auto if target > options.ef_search => Some(SearchReason::LargeK),
            SearchStrategy::Auto => None,
        };
        let mut computations = 0;
        let mut report = if let Some(reason) = reason {
            let mut report = self.exact_subset(&query, target, &eligible, &mut computations);
            report.reason = reason;
            report
        } else if target == 0 {
            self.report(Vec::new(), 0, 0, SearchMode::Hnsw)
        } else {
            let mut entry = self.entry.expect("eligible records imply an entry");
            for layer in (1..self.nodes[entry].links.len()).rev() {
                entry = self.greedy(&query, entry, layer, &mut computations);
            }
            let best = self.search_layer(
                &query,
                entry,
                options.ef_search,
                0,
                Eligibility::Allowed(&allowed),
                &mut computations,
            );
            let mut report = self.report(best, target, computations, SearchMode::Hnsw);
            report.reason = if options.strategy == SearchStrategy::Auto {
                SearchReason::GraphSelected
            } else {
                SearchReason::HnswRequested
            };
            if !report.complete && options.strategy == SearchStrategy::Auto {
                report = self.exact_subset(&query, target, &eligible, &mut computations);
                report.reason = SearchReason::InsufficientGraphCandidates;
            }
            report
        };
        report.eligible_count = eligible.len();
        Ok(report)
    }

    fn exact_subset(
        &self,
        query: &Query<'_>,
        target: usize,
        eligible: &[usize],
        computations: &mut usize,
    ) -> SearchReport {
        let mut best = BinaryHeap::new();
        if target != 0 {
            for &node in eligible {
                let candidate = self.score(query, node, computations);
                if best.len() < target || candidate < *best.peek().unwrap() {
                    best.push(candidate);
                    if best.len() > target {
                        best.pop();
                    }
                }
            }
        }
        self.report(best.into_vec(), target, *computations, SearchMode::Exact)
    }

    pub fn stats(&self) -> IndexStats {
        IndexStats {
            active_records: self.len(),
            physical_nodes: self.nodes.len(),
            tombstones: self.nodes.len() - self.len(),
            layers: self.entry.map_or(0, |e| self.nodes[e].links.len()),
            directed_edges: self.nodes.iter().flat_map(|n| &n.links).map(Vec::len).sum(),
            vector_bytes: self.nodes.len() * self.config.dimensions * 4,
        }
    }

    pub fn compact(&mut self) -> Result<usize> {
        let replacement = self.compacted()?;
        let removed = self.nodes.len() - replacement.nodes.len();
        *self = replacement;
        Ok(removed)
    }

    pub(crate) fn compacted(&self) -> Result<Self> {
        Self::from_records(
            self.config.clone(),
            self.nodes
                .iter()
                .filter(|n| !n.record.deleted)
                .map(|n| n.record.clone())
                .collect(),
        )
    }

    pub(crate) fn records(&self) -> impl Iterator<Item = &Record> {
        self.nodes.iter().map(|n| &n.record)
    }

    pub(crate) fn from_records(config: Config, records: Vec<Record>) -> Result<Self> {
        let mut result = Self::new(config)?;
        for record in records {
            result.validate_put(&record.vector, &record.metadata)?;
            if !record.deleted && result.active.contains_key(&record.id) {
                return Err(Error::Corrupt("duplicate active ID in snapshot".into()));
            }
            let id = record.id;
            let active = !record.deleted;
            let node = result.append_node(record);
            if active {
                result.active.insert(id, node);
            }
        }
        Ok(result)
    }

    pub(crate) fn graph(&self) -> GraphState {
        GraphState {
            links: self.nodes.iter().map(|n| n.links.clone()).collect(),
            entry: self.entry,
            rng_state: self.rng.state(),
        }
    }

    pub(crate) fn from_cached_graph(
        config: Config,
        records: Vec<Record>,
        graph: GraphState,
    ) -> Result<Self> {
        let mut result = Self::new(config)?;
        let cached_count = graph.links.len();
        if cached_count > records.len() {
            return Err(Error::Corrupt("cache has more nodes than records".into()));
        }
        let mut records = records.into_iter();
        for links in graph.links {
            let record = records.next().unwrap();
            result.validate_put(&record.vector, &record.metadata)?;
            result.encoded_bytes += record.encoded_len();
            let norm = if result.config.metric == Metric::Cosine {
                math::inverse_norm(&record.vector)
            } else {
                0.0
            };
            if !record.deleted
                && result
                    .active
                    .insert(record.id, result.nodes.len())
                    .is_some()
            {
                return Err(Error::Corrupt("duplicate active cache ID".into()));
            }
            result.nodes.push(Node {
                record,
                links,
                inverse_norm: norm,
            });
        }
        result.entry = graph.entry;
        result.rng = Rng::new(graph.rng_state);
        result.check_invariants()?;
        for record in records {
            result.validate_put(&record.vector, &record.metadata)?;
            let id = record.id;
            let active = !record.deleted;
            let node = result.append_node(record);
            if active {
                result.active.insert(id, node);
            }
        }
        Ok(result)
    }

    /// Diagnose structural invariants without changing the index.
    pub fn check_invariants(&self) -> Result<()> {
        for (i, node) in self.nodes.iter().enumerate() {
            if !node.record.deleted && self.active.get(&node.record.id) != Some(&i) {
                return Err(Error::Corrupt("active ID mapping mismatch".into()));
            }
            for (layer, neighbors) in node.links.iter().enumerate() {
                if neighbors.len() > self.degree_limit(layer) {
                    return Err(Error::Corrupt("graph degree limit exceeded".into()));
                }
                for (position, &neighbor) in neighbors.iter().enumerate() {
                    if neighbor == i
                        || neighbor >= self.nodes.len()
                        || self.nodes[neighbor].links.len() <= layer
                        || neighbors[..position].contains(&neighbor)
                    {
                        return Err(Error::Corrupt("invalid graph edge".into()));
                    }
                }
            }
        }
        if let Some(entry) = self.entry {
            if entry >= self.nodes.len() {
                return Err(Error::Corrupt("cache entry out of bounds".into()));
            }
            if self
                .nodes
                .iter()
                .any(|n| n.links.len() > self.nodes[entry].links.len())
            {
                return Err(Error::Corrupt("entry is below the highest layer".into()));
            }
        } else if !self.nodes.is_empty() {
            return Err(Error::Corrupt("missing entry point".into()));
        }
        Ok(())
    }

    fn report(
        &self,
        mut scores: Vec<Scored>,
        target: usize,
        computations: usize,
        mode: SearchMode,
    ) -> SearchReport {
        scores.sort_unstable();
        scores.truncate(target);
        let neighbors: Vec<_> = scores
            .into_iter()
            .map(|s| Neighbor {
                id: s.id,
                distance: s.distance,
                metadata: self.nodes[s.node].record.metadata.clone(),
            })
            .collect();
        SearchReport {
            complete: neighbors.len() == target,
            neighbors,
            distance_computations: computations,
            mode,
            metric: self.config.metric,
            eligible_count: self.len(),
            reason: if mode == SearchMode::Exact {
                SearchReason::ExactRequested
            } else {
                SearchReason::HnswRequested
            },
        }
    }

    fn score(&self, query: &Query<'_>, node: usize, computations: &mut usize) -> Scored {
        *computations += 1;
        Scored {
            distance: math::distance(
                self.config.metric,
                query,
                &self.nodes[node].record.vector,
                self.nodes[node].inverse_norm,
            ),
            id: self.nodes[node].record.id,
            node,
        }
    }

    fn greedy(
        &self,
        query: &Query<'_>,
        entry: usize,
        layer: usize,
        computations: &mut usize,
    ) -> usize {
        let mut current = self.score(query, entry, computations);
        loop {
            let before = current;
            for &neighbor in &self.nodes[before.node].links[layer] {
                let candidate = self.score(query, neighbor, computations);
                if candidate < current {
                    current = candidate;
                }
            }
            if current.node == before.node {
                return current.node;
            }
        }
    }

    fn search_layer(
        &self,
        query: &Query<'_>,
        entry: usize,
        ef: usize,
        layer: usize,
        eligibility: Eligibility<'_>,
        computations: &mut usize,
    ) -> Vec<Scored> {
        let mut visited = vec![false; self.nodes.len()];
        let first = self.score(query, entry, computations);
        let mut frontier = BinaryHeap::from([Reverse(first)]);
        let mut best: BinaryHeap<Scored> = BinaryHeap::new();
        visited[entry] = true;
        let eligible = |n: usize| match &eligibility {
            Eligibility::All => true,
            Eligibility::Active => !self.nodes[n].record.deleted,
            Eligibility::Allowed(allowed) => allowed[n],
        };
        if eligible(entry) {
            best.push(first);
        }
        while let Some(Reverse(current)) = frontier.pop() {
            if best.len() == ef && current > *best.peek().unwrap() {
                break;
            }
            for &neighbor in &self.nodes[current.node].links[layer] {
                if visited[neighbor] {
                    continue;
                }
                visited[neighbor] = true;
                let candidate = self.score(query, neighbor, computations);
                if best.len() < ef || candidate < *best.peek().unwrap() {
                    frontier.push(Reverse(candidate));
                    if eligible(neighbor) {
                        best.push(candidate);
                        if best.len() > ef {
                            best.pop();
                        }
                    }
                }
            }
        }
        let mut result = best.into_vec();
        result.sort_unstable();
        result
    }

    fn degree_limit(&self, layer: usize) -> usize {
        if layer == 0 {
            2 * self.config.m
        } else {
            self.config.m
        }
    }

    fn select_neighbors(&self, mut candidates: Vec<Scored>, limit: usize) -> Vec<usize> {
        candidates.sort_unstable();
        let mut chosen = Vec::with_capacity(limit);
        let mut pruned = Vec::new();
        for candidate in candidates {
            if chosen.len() == limit {
                break;
            }
            let diverse = chosen.iter().all(|&other: &usize| {
                math::distance(
                    self.config.metric,
                    &Query {
                        vector: &self.nodes[candidate.node].record.vector,
                        inverse_norm: self.nodes[candidate.node].inverse_norm,
                    },
                    &self.nodes[other].record.vector,
                    self.nodes[other].inverse_norm,
                ) >= candidate.distance
            });
            if diverse {
                chosen.push(candidate.node);
            } else {
                pruned.push(candidate.node);
            }
        }
        // keepPrunedConnections: retain useful close links when diversity alone
        // leaves spare capacity, including small early graphs.
        for neighbor in pruned {
            if chosen.len() == limit {
                break;
            }
            chosen.push(neighbor);
        }
        chosen
    }

    fn append_node(&mut self, record: Record) -> usize {
        let level = self.rng.level(self.config.m);
        let node = self.nodes.len();
        let coordinates = record.vector.clone();
        let query = Query::new(&coordinates, self.config.metric);
        self.encoded_bytes += record.encoded_len();
        self.nodes.push(Node {
            record,
            links: vec![Vec::new(); level + 1],
            inverse_norm: query.inverse_norm,
        });
        let Some(mut entry) = self.entry else {
            self.entry = Some(node);
            return node;
        };
        let highest = self.nodes[entry].links.len() - 1;
        let mut computations = 0;
        if level < highest {
            for layer in ((level + 1)..=highest).rev() {
                entry = self.greedy(&query, entry, layer, &mut computations);
            }
        }
        for layer in (0..=level.min(highest)).rev() {
            let candidates = self.search_layer(
                &query,
                entry,
                self.config.ef_construction,
                layer,
                Eligibility::All,
                &mut computations,
            );
            let next_entry = candidates.first().map_or(entry, |c| c.node);
            let neighbors = self.select_neighbors(candidates, self.config.m);
            self.nodes[node].links[layer] = neighbors.clone();
            for neighbor in neighbors {
                let mut links = self.nodes[neighbor].links[layer].clone();
                links.push(node);
                if links.len() > self.degree_limit(layer) {
                    let center = Query {
                        vector: &self.nodes[neighbor].record.vector,
                        inverse_norm: self.nodes[neighbor].inverse_norm,
                    };
                    let candidates = links
                        .iter()
                        .map(|&n| self.score(&center, n, &mut computations))
                        .collect();
                    links = self.select_neighbors(candidates, self.degree_limit(layer));
                }
                self.nodes[neighbor].links[layer] = links;
            }
            entry = next_entry;
        }
        if level > highest {
            self.entry = Some(node);
        }
        node
    }
}
