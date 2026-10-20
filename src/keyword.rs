use crate::{Document, Error, Result, SearchOptions, SearchReport, config::MAX_EF};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
};

/// Token policy for the disposable keyword index. Select it on every open;
/// original documents, vectors and persisted formats are unchanged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeywordAnalyzer {
    /// Lowercased Unicode words, preserving underscores and hyphens.
    #[default]
    Words,
    /// Words plus two/three-character Hangul syllable grams. Document length
    /// remains the original word count, so queries without Hangul retain Words
    /// scores. Partial Korean matching can introduce unrelated partial matches.
    KoreanNgram,
}

/// Original document ranked by lexical BM25 score, descending, then ID.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct KeywordNeighbor {
    /// Stored original chunk, source and tags.
    pub document: Document,
    /// BM25 with k1=1.2, b=0.75 and whole-collection document frequencies.
    pub score: f64,
}

/// Exact lexical ranking over documents matching at least one query token.
#[derive(Debug)]
#[non_exhaustive]
pub struct KeywordSearchReport {
    /// Highest BM25 scores, with ascending IDs breaking ties.
    pub neighbors: Vec<KeywordNeighbor>,
    /// Active documents satisfying source/tag filters, including nonmatches.
    pub eligible_count: usize,
    /// Eligible documents matching at least one indexed query token.
    pub matching_count: usize,
    /// Term/document postings examined, including those rejected by a filter.
    pub postings_visited: usize,
}

/// Candidate depths and reciprocal-rank-fusion settings for document search.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct HybridSearchOptions {
    /// Execution policy for the vector branch.
    pub vector: SearchOptions,
    /// Maximum candidates from each branch, in 1..=4096. K must not exceed it.
    pub candidates: usize,
    /// Positive rank offset in 1..=4096; score is sum(1/(offset + one-based rank)).
    pub rrf_k: usize,
}

impl Default for HybridSearchOptions {
    fn default() -> Self {
        Self {
            vector: SearchOptions::default(),
            candidates: 64,
            rrf_k: 60,
        }
    }
}

impl HybridSearchOptions {
    /// Set the vector branch policy and work limits.
    pub fn with_vector_options(mut self, options: SearchOptions) -> Self {
        self.vector = options;
        self
    }
    /// Set each branch's ranking depth.
    pub fn with_candidates(mut self, candidates: usize) -> Self {
        self.candidates = candidates;
        self
    }
    /// Set the positive reciprocal rank offset.
    pub fn with_rrf_k(mut self, rrf_k: usize) -> Self {
        self.rrf_k = rrf_k;
        self
    }
    /// Validate settings and requested K before search.
    pub fn validate(&self, k: usize) -> Result<()> {
        self.vector.validate()?;
        if !(1..=MAX_EF).contains(&self.candidates)
            || !(1..=MAX_EF).contains(&self.rrf_k)
            || k > self.candidates
        {
            return Err(Error::InvalidInput(format!(
                "hybrid candidates and RRF offset must be 1..={MAX_EF}; K must not exceed candidates"
            )));
        }
        Ok(())
    }
}

/// Original document ranked by fused vector and keyword ranks.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct HybridNeighbor {
    /// Stored original chunk, source and tags.
    pub document: Document,
    /// Descending reciprocal rank fusion score; ascending ID breaks ties.
    pub score: f64,
    /// One-based vector rank; None when absent from that candidate list.
    pub vector_rank: Option<usize>,
    /// One-based BM25 rank; None when absent from that candidate list.
    pub keyword_rank: Option<usize>,
    /// Vector distance when present in the vector candidate list.
    pub distance: Option<f64>,
    /// BM25 score when present in the keyword candidate list.
    pub keyword_score: Option<f64>,
}

/// Fused candidate results and the work performed by both branches.
#[derive(Debug)]
#[non_exhaustive]
pub struct HybridSearchReport {
    /// Fused ranking, truncated to K.
    pub neighbors: Vec<HybridNeighbor>,
    /// Vector execution, completeness and work counters before fusion.
    pub vector: SearchReport,
    /// Keyword candidates, match counts and posting work before fusion.
    pub keyword: KeywordSearchReport,
    /// Number of distinct documents in the two candidate lists before truncation.
    pub candidate_count: usize,
    /// Whether min(K, eligible documents) results were returned; not a recall claim.
    pub complete: bool,
    /// Rank offset used to compute scores.
    pub rrf_k: usize,
}

#[derive(Default, PartialEq)]
pub(crate) struct KeywordIndex {
    analyzer: KeywordAnalyzer,
    terms: BTreeMap<String, BTreeMap<u64, usize>>,
    lengths: BTreeMap<u64, usize>,
    total_tokens: usize,
}

/// Build disposable postings in bulk on open. Collecting sorted ID/count rows
/// fills B-tree nodes instead of leaving the spare nodes produced by repeated
/// ascending insertions. Original records remain in the database throughout.
#[derive(Default)]
pub(crate) struct KeywordBuilder {
    analyzer: KeywordAnalyzer,
    terms: BTreeMap<String, Vec<(u64, usize)>>,
    lengths: Vec<(u64, usize)>,
    total_tokens: usize,
}

impl KeywordBuilder {
    pub(crate) fn with_analyzer(analyzer: KeywordAnalyzer) -> Self {
        Self {
            analyzer,
            ..Self::default()
        }
    }

    pub(crate) fn add(&mut self, id: u64, text: &str) {
        let (terms, length) = frequencies(text, self.analyzer);
        self.lengths.push((id, length));
        self.total_tokens += length;
        for (term, count) in terms {
            if let Some(postings) = self.terms.get_mut(term.as_ref()) {
                postings.push((id, count));
            } else {
                self.terms.insert(term.into_owned(), vec![(id, count)]);
            }
        }
    }

    pub(crate) fn finish(self) -> KeywordIndex {
        KeywordIndex {
            analyzer: self.analyzer,
            terms: self
                .terms
                .into_iter()
                .map(|(term, rows)| (term, rows.into_iter().collect()))
                .collect(),
            lengths: self.lengths.into_iter().collect(),
            total_tokens: self.total_tokens,
        }
    }
}

// Preserve snake_case and hyphenated error/product identifiers. Unicode case
// folding here is lowercase only: no stemming, morphology or normalization.
fn words(text: &str) -> impl Iterator<Item = Cow<'_, str>> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter(|token| token.chars().any(char::is_alphanumeric))
        .map(|token| {
            if token.is_ascii() && !token.bytes().any(|b| b.is_ascii_uppercase()) {
                Cow::Borrowed(token)
            } else {
                Cow::Owned(token.to_lowercase())
            }
        })
}

fn word_frequencies(text: &str) -> BTreeMap<Cow<'_, str>, usize> {
    let mut result = BTreeMap::new();
    for word in words(text) {
        *result.entry(word).or_default() += 1;
    }
    result
}

fn frequencies(text: &str, analyzer: KeywordAnalyzer) -> (BTreeMap<Cow<'_, str>, usize>, usize) {
    let words = word_frequencies(text);
    // Grams are alternate features of the same lexical positions. Counting
    // them as extra length penalizes Korean-heavy documents even for English
    // queries whose term frequencies and document frequencies never changed.
    let length = words.values().sum();
    if analyzer == KeywordAnalyzer::Words {
        return (words, length);
    }
    let mut result = BTreeMap::new();
    for (word, count) in words {
        let mut grams = BTreeSet::new();
        for run in word.split(|c: char| !('\u{ac00}'..='\u{d7a3}').contains(&c)) {
            let chars: Vec<_> = run.chars().collect();
            for width in [2, 3] {
                for part in chars.windows(width) {
                    let gram: String = part.iter().collect();
                    if gram != word {
                        grams.insert(gram);
                    }
                }
            }
        }
        for gram in grams {
            *result.entry(Cow::Owned(gram)).or_default() += count;
        }
        *result.entry(word).or_default() += count;
    }
    (result, length)
}

pub(crate) fn validate_query(text: &str) -> Result<()> {
    if text.len() > 6000 || text.contains('\0') {
        return Err(Error::InvalidInput(
            "keyword query must be at most 6000 UTF-8 bytes without NUL".into(),
        ));
    }
    Ok(())
}

impl KeywordIndex {
    pub(crate) fn analyzer(&self) -> KeywordAnalyzer {
        self.analyzer
    }

    pub(crate) fn add(&mut self, id: u64, text: &str) {
        if self.analyzer == KeywordAnalyzer::Words {
            // Incremental word updates avoid temporary frequency maps. Borrow
            // already-lowercase ASCII until the vocabulary needs an owned term,
            // rather than allocating another String for every occurrence.
            let mut length = 0;
            for word in words(text) {
                length += 1;
                if let Some(postings) = self.terms.get_mut(word.as_ref()) {
                    *postings.entry(id).or_default() += 1;
                } else {
                    self.terms
                        .insert(word.into_owned(), BTreeMap::from([(id, 1)]));
                }
            }
            self.lengths.insert(id, length);
            self.total_tokens += length;
            return;
        }
        let (terms, length) = frequencies(text, self.analyzer);
        self.lengths.insert(id, length);
        self.total_tokens += length;
        for (term, count) in terms {
            if let Some(postings) = self.terms.get_mut(term.as_ref()) {
                postings.insert(id, count);
            } else {
                self.terms
                    .insert(term.into_owned(), BTreeMap::from([(id, count)]));
            }
        }
    }

    pub(crate) fn remove(&mut self, id: u64, text: &str) {
        if let Some(length) = self.lengths.remove(&id) {
            self.total_tokens -= length;
            for term in frequencies(text, self.analyzer).0.keys() {
                if let Some(postings) = self.terms.get_mut(term.as_ref()) {
                    postings.remove(&id);
                    if postings.is_empty() {
                        self.terms.remove(term.as_ref());
                    }
                }
            }
        }
    }

    pub(crate) fn rank(
        &self,
        text: &str,
        k: usize,
        allowed: Option<&BTreeSet<u64>>,
    ) -> (Vec<(u64, f64)>, usize, usize) {
        let mut scores = BTreeMap::<u64, f64>::new();
        let mut visited = 0;
        if self.total_tokens == 0 {
            return (Vec::new(), 0, 0);
        }
        let n = self.lengths.len() as f64;
        let average = self.total_tokens as f64 / n;
        for term in frequencies(text, self.analyzer).0.keys() {
            let Some(postings) = self.terms.get(term.as_ref()) else {
                continue;
            };
            let df = postings.len() as f64;
            let idf = (1.0 + (n - df + 0.5) / (df + 0.5)).ln();
            for (&id, &count) in postings {
                visited += 1;
                if allowed.is_some_and(|set| !set.contains(&id)) {
                    continue;
                }
                let tf = count as f64;
                let length = self.lengths[&id] as f64;
                let score = idf * tf * 2.2 / (tf + 1.2 * (0.25 + 0.75 * length / average));
                *scores.entry(id).or_default() += score;
            }
        }
        let matching = scores.len();
        let mut ranked = scores.into_iter().collect::<Vec<_>>();
        ranked.sort_unstable_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        ranked.truncate(k);
        (ranked, matching, visited)
    }
}

pub(crate) fn fuse(
    vector: &crate::DocumentSearchReport,
    keyword: &KeywordSearchReport,
    rrf_k: usize,
    k: usize,
) -> (Vec<HybridNeighbor>, usize) {
    struct Candidate<'a> {
        document: &'a Document,
        score: f64,
        vector_rank: Option<usize>,
        keyword_rank: Option<usize>,
        distance: Option<f64>,
        keyword_score: Option<f64>,
    }
    let mut merged = BTreeMap::<u64, Candidate<'_>>::new();
    for (rank, neighbor) in vector.neighbors.iter().enumerate() {
        merged.insert(
            neighbor.document.id,
            Candidate {
                document: &neighbor.document,
                score: 1.0 / (rrf_k as f64 + rank as f64 + 1.0),
                vector_rank: Some(rank + 1),
                keyword_rank: None,
                distance: Some(neighbor.distance),
                keyword_score: None,
            },
        );
    }
    for (rank, neighbor) in keyword.neighbors.iter().enumerate() {
        let fused = merged
            .entry(neighbor.document.id)
            .or_insert_with(|| Candidate {
                document: &neighbor.document,
                score: 0.0,
                vector_rank: None,
                keyword_rank: None,
                distance: None,
                keyword_score: None,
            });
        fused.keyword_rank = Some(rank + 1);
        fused.keyword_score = Some(neighbor.score);
        fused.score += 1.0 / (rrf_k as f64 + rank as f64 + 1.0);
    }
    let count = merged.len();
    let mut ranked = merged.into_values().collect::<Vec<_>>();
    ranked.sort_unstable_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(a.document.id.cmp(&b.document.id))
    });
    ranked.truncate(k);
    (
        ranked
            .into_iter()
            .map(|c| HybridNeighbor {
                document: c.document.clone(),
                score: c.score,
                vector_rank: c.vector_rank,
                keyword_rank: c.keyword_rank,
                distance: c.distance,
                keyword_score: c.keyword_score,
            })
            .collect(),
        count,
    )
}
