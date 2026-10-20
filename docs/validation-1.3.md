# 1.3 candidate validation

The 1.3 candidate adds document-client and owning-worker features while retaining
the default tokenization, vector math and persisted snapshot/WAL/cache formats.
Stable promotion requires the exact registered RC's completed 24-hour workload,
100k acknowledged mutations and registry distribution receipts, including actual
Windows 11 NTFS. These gates are pending until successful receipts are recorded.

## Local correctness

New checks cover read-only inspection of an incomplete WAL without byte changes,
unchanged/metadata-only/chunk-level embedding reuse, empty and explained output,
environment errors, Korean analyzer mutation/reopen behavior, all worker retrieval
modes, status, stale writes, compaction and independent backups. Context assertions
reject excluded documents even when they are adjacent to an eligible result.
JSONL checks retain full-u64 string IDs, original text and signed-zero f32 vectors,
and reject invalid/duplicate records before claiming a destination. Python custom
embedding and native error contracts run against the actual local CLI.

The native archive runner extracts each ZIP and verifies its version, offline
demo, restart and embedded client help. Archive build host/target and checksums
are recorded separately from registry distribution receipts.

## Authored relevance cases

`tools/evaluate_relevance.py` predeclares 70 authored documents and 230 questions:
100 Korean prose, 50 English prose, 50 exact identifiers and 30 unrelated queries.
The prose uses templates over 20 authored bilingual topics; identifier documents
use fictional error codes. Labels are authored from the document topics, not
independent human judgments. Every fifth question is held out from the development
subset; no query-policy tuning is performed using validation output.

The initial local run used genuine local EmbeddingGemma embeddings and 768
dimensions. Default vector/keyword/hybrid/auto native responses exactly matched
the fresh 1.2 source binary on identical stored inputs. Among 200 answerable cases,
vector/auto Hit@1 was 100%, default keyword 75%, hybrid 86% and optional Korean
keyword 87%. Keyword source Recall@10 increased from 85% to 97.5% with grams.
This authored fixture does not establish general or production multilingual quality.

All 30 unrelated queries still returned vector/hybrid candidates; keyword returned
none. Returning K candidates does not establish that the corpus answers a question.
The optional distance bound remains application-calibrated, and `complete` remains
a count observation. Default retrieval and tokenization are unchanged.

Run the existing 22-case evaluation as well as the expanded cases on the final
candidate, retaining model digest, input/judgment/binary hashes and raw responses.
Local timing and resource comparisons must use sequential fresh processes with
identical datasets/settings, and separate model work, process open, queue wait and
engine search. No competitor performance ranking is claimed by this validation.

## External observations

The owner confirmed no external participants/apps are available for the proposed
five-person first-use study. External first-use completion and repeat adoption
remain unmeasured; deterministic integration tests are technical evidence only.
This does not satisfy that product research goal and is not presented as user
acceptance. Existing release policy treats adoption observations separately from
the technical publication gate.
