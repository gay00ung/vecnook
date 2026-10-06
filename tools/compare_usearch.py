#!/usr/bin/env python3
"""Independent f64 oracle and optional single-threaded USearch comparison."""
import argparse
import json
from pathlib import Path
import resource
import sys
import time

import numpy as np
import usearch
from usearch.index import Index


def read_fvecs(path):
    size = Path(path).stat().st_size
    if size > 256 * 1024 * 1024 or size % 4:
        raise ValueError("fvecs size exceeds 256 MiB or has a partial float")
    raw = np.fromfile(path, dtype="<f4")
    if not len(raw):
        raise ValueError("empty fvecs")
    dimensions = int(raw[:1].view("<u4")[0])
    if not 1 <= dimensions <= 4096 or len(raw) % (dimensions + 1):
        raise ValueError("invalid fvecs dimensions/length")
    rows = raw.reshape(-1, dimensions + 1)
    if len(rows) > 100_000 or not np.all(rows[:, 0].view("<u4") == dimensions):
        raise ValueError("invalid fvecs row count/headers")
    vectors = np.ascontiguousarray(rows[:, 1:])
    if not np.all(np.isfinite(vectors)) or np.any(np.all(vectors == 0, axis=1)):
        raise ValueError("invalid cosine coordinates")
    return vectors


def oracle(corpus, queries):
    values = corpus.astype(np.float64)
    norms = np.sqrt(np.sum(values * values, axis=1))
    ids = np.arange(len(corpus))
    results = []
    for query in queries.astype(np.float64):
        distances = np.clip(1.0 - np.sum(values * query, axis=1) / (norms * np.sqrt(np.sum(query * query))), 0, 2)
        top = np.lexsort((ids, distances))[:min(10, len(ids))]
        results.append([{"id": int(id), "distance": float(distances[id])} for id in top])
    return results


def run(args):
    corpus = read_fvecs(args.base)
    queries = read_fvecs(args.query)
    if corpus.shape[1] != queries.shape[1] or len(queries) > 10_000:
        raise ValueError("corpus/query mismatch or too many queries")
    if args.oracle:
        truth = oracle(corpus, queries)
        if args.rust_truth:
            rust = json.loads(Path(args.rust_truth).read_text())
            if len(rust) != len(truth):
                raise ValueError("Rust oracle query count mismatch")
            for actual, expected in zip(rust, truth):
                if [n["id"] for n in actual] != [n["id"] for n in expected]:
                    raise ValueError("Rust/independent Top-K IDs differ (inspect boundary ties)")
                if any(abs(a["distance"] - e["distance"]) > 1e-12 for a, e in zip(actual, expected)):
                    raise ValueError("Rust/independent distances differ")
        Path(args.oracle).write_text(json.dumps(truth) + "\n")
        print(json.dumps({"independent_oracle": True, "queries": len(truth), "k": len(truth[0]), "rust_verified": bool(args.rust_truth)}))
        return
    truth = json.loads(Path(args.truth).read_text())
    if len(truth) != len(queries):
        raise ValueError("truth query count differs")
    k = min(10, len(corpus))
    if not k <= args.ef <= 4096:
        raise ValueError("ef outside min(10,count)..4096")
    index = Index(ndim=corpus.shape[1], metric="cos", dtype="f32", connectivity=16,
                  expansion_add=200, expansion_search=args.ef)
    start = time.perf_counter()
    index.add(np.arange(len(corpus), dtype=np.uint64), corpus, threads=1)
    build_seconds = time.perf_counter() - start
    for query in queries[:10]:
        index.search(query, count=k, threads=1)
    samples, hits, incomplete = [], 0, 0
    for query, expected in zip(queries, truth):
        start = time.perf_counter()
        matched = index.search(query, count=k, threads=1)
        samples.append(time.perf_counter() - start)
        keys = set(map(int, matched.keys))
        hits += len(keys & {n["id"] for n in expected})
        incomplete += int(len(keys) != k)
    peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    peak_bytes = int(peak if sys.platform == "darwin" else peak * 1024)
    ordered = sorted(samples)
    def percentile(percent):
        import math
        return ordered[max(0, math.ceil(len(ordered) * percent) - 1)] * 1000
    print(json.dumps({"engine": "USearch Python", "version": usearch.__version__, "count": len(corpus),
                      "queries": len(queries), "dimensions": corpus.shape[1], "k": k, "ef": args.ef,
                      "metric": "cosine", "dtype": "f32", "connectivity": 16, "expansion_add": 200,
                      "threads": 1, "build_seconds": build_seconds, "recall": hits/(len(queries)*k),
                      "incomplete_queries": incomplete, "p50_ms": percentile(.5), "p95_ms": percentile(.95),
                      "sequential_qps": len(queries)/sum(samples), "peak_process_rss_bytes": peak_bytes,
                      "rss_scope": "Python+NumPy+binding+input arrays+native index; oracle generated separately"}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("base")
    parser.add_argument("query")
    parser.add_argument("--ef", type=int, default=128)
    parser.add_argument("--truth")
    parser.add_argument("--oracle", help="write independent Top-K ground truth")
    parser.add_argument("--rust-truth", help="verify every ID and distance against Rust output")
    args = parser.parse_args()
    if not args.oracle and not args.truth:
        parser.error("provide --oracle or --truth")
    try:
        run(args)
    except (ValueError, OSError, KeyError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
