#!/usr/bin/env python3
"""Run a fixed-configuration Vecnook/USearch/Chroma comparison sequentially."""
import argparse
from datetime import datetime, timezone
from importlib import metadata
import json
import os
from pathlib import Path
import statistics
import subprocess
import sys

from benchmark_readme import ROOT, output, sha256


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("base", type=Path)
    parser.add_argument("query", type=Path)
    parser.add_argument("--truth", type=Path, required=True)
    parser.add_argument("--rust-truth", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error("choose a fresh output path")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    for key in ["OMP_NUM_THREADS", "OPENBLAS_NUM_THREADS", "MKL_NUM_THREADS", "NUMEXPR_NUM_THREADS"]:
        os.environ[key] = "1"
    os.environ["ANONYMIZED_TELEMETRY"] = "False"
    binary = ROOT / "target/release/vecnook"
    identity = sha256(binary)
    version = output(str(binary), "--version").removeprefix("vecnook ")
    # Establish a shared independent oracle before accepting any timed results.
    independent = json.loads(args.truth.read_text())
    rust = json.loads(args.rust_truth.read_text())
    if len(independent) != 300 or len(rust) != 300:
        raise ValueError("Expected 300 independent oracle queries")
    for exact, reference in zip(rust, independent):
        if len(exact) != 10 or len(reference) != 10:
            raise ValueError("Expected exact Top-10")
        if [n["id"] for n in exact] != [n["id"] for n in reference]:
            raise ValueError("Rust and independent oracle IDs differ")
        if any(abs(a["distance"] - b["distance"]) > 1e-12 for a, b in zip(exact, reference)):
            raise ValueError("Rust and independent oracle distances differ")
    evidence = {
        "schema": 1, "started_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_revision": output("git", "rev-parse", "HEAD"),
        "vecnook_version": version, "binary_sha256": identity,
        "environment": {"cpu": output("sysctl", "-n", "machdep.cpu.brand_string"),
                        "memory_bytes": int(output("sysctl", "-n", "hw.memsize")),
                        "os": output("sw_vers", "-productVersion"),
                        "os_build": output("sw_vers", "-buildVersion"),
                        "rustc": output("rustc", "--version"), "python": sys.version.split()[0],
                        "packages": sorted(f"{d.metadata['Name']}=={d.version}" for d in metadata.distributions())},
        "dataset": {"name": "BEIR SciFact", "count": 5183, "dimensions": 768, "queries": 300,
                    "base_sha256": sha256(args.base), "query_sha256": sha256(args.query),
                    "independent_oracle_sha256": sha256(args.truth),
                    "rust_exact_sha256": sha256(args.rust_truth), "oracle_verified": True},
        "method": {"rounds": 3, "ef": 128, "m": 16, "ef_construction": 200,
                   "metric": "cosine", "dtype": "f32", "k": 10, "warmup_queries": 10,
                   "hnsw_threads": 1, "query_mode": "sequential single-query calls",
                   "selection": "fixed efSearch=128 before comparison; no per-engine tuning",
                   "seed": {"Vecnook": 42, "USearch": "not exposed", "Chroma": "not exposed"},
                   "scope": "warm query-call latency with result construction; API scopes differ",
                   "exclusions": ["embeddings", "input loading", "graph construction", "process startup",
                                  "network", "concurrent clients", "memory comparison"],
                   "host_activity": "other host activity uncontrolled; no driver build/plot jobs overlap"},
        "runs": [],
    }
    base_query = [str(args.base.resolve()), str(args.query.resolve()), "--ef", "128"]
    commands = {
        "Vecnook": ["measure_vecnook.py", *base_query, "--binary", str(binary)],
        "USearch": ["compare_usearch.py", *base_query, "--truth", str(args.truth.resolve())],
        "Chroma": ["compare_chroma.py", *base_query, "--truth", str(args.truth.resolve())],
    }
    names = list(commands)
    for round_index in range(3):
        for name in names[round_index:] + names[:round_index]:
            command = commands[name]
            child = subprocess.run([sys.executable, str(ROOT / "tools" / command[0]), *command[1:]],
                                   cwd=ROOT, text=True, capture_output=True, check=True, timeout=600)
            stdout = child.stdout.strip()
            result = json.loads(stdout)
            if any(result[k] != v for k, v in {"count": 5183, "queries": 300, "dimensions": 768,
                                              "k": 10, "ef": 128, "incomplete_queries": 0}.items()):
                raise ValueError(f"Unexpected benchmark result: {result}")
            if name == "Vecnook" and result["version"] != version:
                raise ValueError("Vecnook version changed")
            result.update(round=round_index + 1, name=name, child_stdout=stdout,
                          child_stderr=child.stderr)
            evidence["runs"].append(result)
            args.output.write_text(json.dumps(evidence, indent=2) + "\n")
            print(f"round={round_index+1} engine={name} p95_ms={result['p95_ms']:.6f} "
                  f"recall={result['recall']*100:.4f}%", flush=True)
    if sha256(binary) != identity:
        raise ValueError("Measured binary changed")
    evidence["summary"] = []
    for name in names:
        samples = [r for r in evidence["runs"] if r["name"] == name]
        row = {"name": name, "engine": samples[0]["engine"], "version": samples[0]["version"]}
        for field in ["recall", "p50_ms", "p95_ms", "sequential_qps"]:
            row[field] = statistics.median(r[field] for r in samples)
        row.update(p95_min_ms=min(r["p95_ms"] for r in samples),
                   p95_max_ms=max(r["p95_ms"] for r in samples),
                   recall_min=min(r["recall"] for r in samples), recall_max=max(r["recall"] for r in samples))
        evidence["summary"].append(row)
    evidence["completed_at_utc"] = datetime.now(timezone.utc).isoformat()
    args.output.write_text(json.dumps(evidence, indent=2) + "\n")


if __name__ == "__main__":
    main()
