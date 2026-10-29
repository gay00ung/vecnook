#!/usr/bin/env python3
"""Measure Chroma's local persistent query API on precomputed SciFact vectors."""
import argparse
import json
import math
import tempfile
import time

import chromadb
from chromadb.config import Settings

from compare_usearch import read_fvecs


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("base")
    parser.add_argument("query")
    parser.add_argument("--truth", required=True)
    parser.add_argument("--ef", type=int, default=128)
    args = parser.parse_args()
    corpus, queries = read_fvecs(args.base), read_fvecs(args.query)
    truth = json.load(open(args.truth))
    if len(truth) != len(queries) or corpus.shape[1] != queries.shape[1]:
        raise ValueError("corpus/query/oracle mismatch")
    k = 10
    if len(corpus) != 5183 or len(queries) != 300 or corpus.shape[1] != 768:
        raise ValueError("Expected frozen 5183 x 768 SciFact corpus and 300 queries")
    hnsw = {"space": "cosine", "max_neighbors": 16, "ef_construction": 200,
            "ef_search": args.ef, "num_threads": 1,
            "batch_size": len(corpus), "sync_threshold": len(corpus)}
    with tempfile.TemporaryDirectory(prefix="vecnook-chroma-comparison-") as directory:
        client = chromadb.PersistentClient(directory, settings=Settings(anonymized_telemetry=False))
        collection = client.create_collection("scifact-comparison", embedding_function=None,
                                              configuration={"hnsw": hnsw})
        embeddings = corpus.tolist()
        # Materialize Python payloads before timing the query call.
        query_payloads = [[q.tolist()] for q in queries]
        start = time.perf_counter()
        batch = client.get_max_batch_size()
        for first in range(0, len(corpus), batch):
            last = min(first + batch, len(corpus))
            collection.add(ids=[str(i) for i in range(first, last)], embeddings=embeddings[first:last])
        build_seconds = time.perf_counter() - start
        assert collection.count() == len(corpus)
        schema = collection.get_model().model_dump()["serialized_schema"]
        vector_config = schema["keys"]["#embedding"]["float_list"]["vector_index"]["config"]
        config = {"hnsw": {**vector_config["hnsw"], "space": vector_config["space"]}}
        if any(config["hnsw"].get(key) != value for key, value in hnsw.items()):
            raise ValueError(f"Chroma did not retain requested configuration: {config}")
        def search(payload):
            return collection.query(query_embeddings=payload, n_results=k, include=["distances"])
        for payload in query_payloads[:10]:
            search(payload)
        times, hits, incomplete = [], 0, 0
        for payload, expected in zip(query_payloads, truth):
            start = time.perf_counter()
            result = search(payload)
            times.append(time.perf_counter() - start)
            ids = [int(value) for value in result["ids"][0]]
            if len(ids) != len(set(ids)) or any(i < 0 or i >= len(corpus) for i in ids):
                raise ValueError("Duplicate or invalid result IDs")
            hits += len(set(ids) & {r["id"] for r in expected})
            incomplete += int(len(ids) != k)
        ordered = sorted(times)
        result = {"engine": "Chroma Python local DB", "version": chromadb.__version__,
                  "count": len(corpus), "queries": len(queries), "dimensions": corpus.shape[1],
                  "k": k, "ef": args.ef, "metric": "cosine", "dtype": "f32",
                  "threads": 1, "configuration": config, "build_seconds": build_seconds,
                  "build_scope": "persistent collection add, including DB ingestion and sync",
                  "recall": hits / (len(queries) * k), "incomplete_queries": incomplete,
                  "p50_ms": ordered[math.ceil(len(times) * .5) - 1] * 1000,
                  "p95_ms": ordered[math.ceil(len(times) * .95) - 1] * 1000,
                  "sequential_qps": len(times) / sum(times),
                  "query_scope": "local collection.query; validation, binding, DB query and distances/IDs response"}
        print(json.dumps(result))


if __name__ == "__main__":
    main()
