# Search document meaning and exact identifiers

See [candidate validation results](validation-1.1.md) for paired latency,
English/Korean relevance and installed first-search measurements.

The 1.1 candidate adds `Collection::search_keywords` (BM25) and `search_hybrid`
(reciprocal rank fusion, RRF). Existing `search` retains its vector result
contract. Source equality and all-tag filters apply before ranking in both
branches. Existing database, WAL, graph-cache and document formats are unchanged.

```rust
use vecnook::{Collection, DocumentFilter, HybridSearchOptions};

fn find(collection: &Collection, embedding: &[f32]) -> vecnook::Result<()> {
    let report = collection.search_hybrid(
        embedding, "write_batch_if_sequence", 10,
        HybridSearchOptions::default(),
        DocumentFilter::default().with_tags(&["rust"]),
    )?;
    for matched in report.neighbors {
        println!("{}:{} score={} vector_rank={:?} keyword_rank={:?}",
            matched.document.source, matched.document.start_line,
            matched.score, matched.vector_rank, matched.keyword_rank);
    }
    Ok(())
}
```

## Choose a ranking

| Ranking | Use it for | Result and cost |
| --- | --- | --- |
| Vector | Prose questions and paraphrases, including Korean questions over English text | Ascending embedding distance; requires a compatible query embedding |
| Keyword | Exact words, function names, error codes and product names | Descending BM25 score; no embedding needed; returns only documents matching a token |
| Hybrid | Explicit identifiers with useful semantic context | Descending RRF score; runs both branches and merges their candidate lists |

RRF does not guarantee better relevance. In the initial evaluation, adding lexical
matches moved some documents above more relevant vector results. `docs-find`
therefore defaults to `auto`: prose uses vector ranking; a single ASCII term or a
query containing an underscore, `::`, letters plus digits, or an upper/lower/upper
product name uses hybrid ranking. `--mode vector|keyword|hybrid` overrides it.
Auto JSON includes `retrieval_mode`. This text heuristic is separate from vector
`SearchStrategy::Auto`. The existing Python client's `search` command retains its
vector default; Rust callers explicitly choose a method.

## Ranking contracts

Tokens are Unicode alphanumeric runs retaining underscores/hyphens, then
lowercased. `search_metadata`, `ERR-42` and `백업` remain whole tokens. Only chunk
text is indexed; sources and tags are filters. There is no substring search,
stemming, Unicode normalization or Korean morphology: `백업` does not match
`백업을`. Repeated query tokens count once. Empty, punctuation-only or unknown
tokens return no matches. Queries accept at most 6000 UTF-8 bytes without NUL;
the Markdown client retains its 3000-byte bound.

BM25 uses k1=1.2, b=0.75 and IDF `ln(1 + (N - df + 0.5)/(df + 0.5))`.
N includes all active documents, including empty text. Lengths and document
frequencies cover the whole collection even with a filter. Reports expose
eligible documents, matching documents and postings visited. Ties use ascending
IDs. K=0 returns no results while retaining lexical match/work counts.

Each hybrid branch supplies at most `candidates` results (default 64, range
1–4096). K must not exceed that depth. The default offset is 60; each present
one-based rank contributes `1/(rrf_k + rank)`, and a missing rank contributes
zero. Results deduplicate IDs, sort by descending fused score then ID, and retain
branch ranks and scores. No lexical matches preserve vector candidate order.
Vector options control exact/HNSW/Auto; forced HNSW needs efSearch at least the
capped candidate depth. `complete` means min(K, eligible documents) results were
returned; it does not certify semantic relevance or ANN recall.

Postings rebuild in RAM from original payloads on open/import. Successful ordered
batches update them after WAL persistence; failed preflight or sequence conflicts
change none of the indexes. Delete/replacement remove old terms. Checkpoint,
compaction, restart, named backup and logical restore preserve rankings without
adding an authoritative storage file. The index adds memory/startup work; reducing
those costs remains 1.2 scope.

## CLI and evaluation

```bash
vecnook docs-sync ./my-markdown --db data/notes
vecnook docs-find "How do I recover my documents?" --db data/notes
vecnook docs-find "Error::Locked" --db data/notes --json
vecnook docs-find "write_batch_if_sequence" --db data/notes --mode keyword
```

Convenience commands embed the existing Python standard-library client in the
binary. They need Python 3.10+; sync and vector/hybrid queries also need local
Ollama. Set `VECNOOK_PYTHON` to a Python executable if necessary. Low-level
`docs-keywords` and `docs-hybrid` use the same root/name/embedding identity prefix
as `docs-search` and emit separate JSON v1 shapes; IDs remain decimal strings.
The library and low-level commands use Rust's standard library only.

```bash
cargo build --release --offline --example filter_bench
target/release/examples/filter_bench > work/filter-run1.jsonl
python3 tools/evaluate_hybrid.py --binary target/release/vecnook --output work/hybrid-run1
```

Repeat in three fresh sequential processes. The filter benchmark compares old/new
Auto on identical 50k/128d vectors with 500 eligible records and an independent
f64 oracle. The text evaluation retains ten repository English/Korean questions,
adds twelve identifier questions, and includes a repetitive error-code catalog.
It records corpus, judgments, vectors and binary hashes plus Hit@1, Recall@3 and
MRR@3 for each ranking and automatic selection. Labels follow the authored
documents; the fixture measures those cases.

[Qdrant's guide](https://qdrant.tech/documentation/search/text-search/hybrid-search/)
and [LanceDB's guide](https://docs.lancedb.com/search/hybrid-search) describe
semantic/lexical retrieval. Vecnook implements its own BM25, tokenizer and RRF
without either engine as a dependency.
