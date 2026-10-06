#!/usr/bin/env python3
"""Measure one native Rust benchmark child and its peak process RSS (POSIX)."""
import argparse
import json
import re
import resource
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("base")
    parser.add_argument("query")
    parser.add_argument("--binary", default="target/release/vecnook")
    parser.add_argument("--ef", type=int, default=128)
    args = parser.parse_args()
    version = subprocess.run([args.binary, "--version"], capture_output=True, text=True, check=True, timeout=30).stdout.strip().removeprefix("vecnook ")
    result = subprocess.run([args.binary, "bench-file", args.base, args.query,
                             "100000", "10000", str(args.ef), "cosine"], capture_output=True,
                            text=True, encoding="utf-8", timeout=600)
    if result.returncode:
        print(result.stderr, file=sys.stderr)
        return 1
    lines = result.stdout.splitlines()
    fields = {key: value for key, value in re.findall(r"([a-z0-9_]+)=([^\s]+)", result.stdout)}
    hnsw = next(line for line in lines if line.startswith("hnsw "))
    metrics = dict(re.findall(r"([a-z0-9_]+)=([^\s]+)", hnsw))
    peak = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    print(json.dumps({"engine": "Vecnook Rust", "version": version, "metric": "cosine",
                      "count": int(fields["count"]), "queries": int(fields["queries"]),
                      "dimensions": int(fields["dimensions"]), "k": int(fields["k"]), "ef": args.ef,
                      "build_seconds": float(fields["build_seconds"]),
                      "recall": float(fields["recall_at_10"].rstrip("%"))/100,
                      "incomplete_queries": int(fields["incomplete_queries"]),
                      "p50_ms": float(metrics["p50_ms"]), "p95_ms": float(metrics["p95_ms"]),
                      "sequential_qps": float(metrics["sequential_qps"]),
                      "peak_process_rss_bytes": int(peak if sys.platform == "darwin" else peak * 1024),
                      "rss_scope": "native benchmark child including loader/query arrays/index/exact oracle",
                      "raw_output": result.stdout}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
