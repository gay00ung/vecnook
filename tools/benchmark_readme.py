#!/usr/bin/env python3
"""Measure the README SciFact sweep sequentially; retain every raw child result.

Build the desired release binary first. Requires only Python's standard library.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import platform
import statistics
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def output(*command):
    return subprocess.check_output(command, text=True, cwd=ROOT).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("base", type=Path)
    parser.add_argument("query", type=Path)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/vecnook")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error("choose a fresh output path to retain previous evidence")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    binary = args.binary.resolve()
    identity = sha256(binary)
    version = output(str(binary), "--version").removeprefix("vecnook ")
    evidence = {
        "schema": 1, "started_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_revision": output("git", "rev-parse", "HEAD"),
        "version": version, "binary_sha256": identity,
        "environment": {"cpu": output("sysctl", "-n", "machdep.cpu.brand_string"),
                        "memory_bytes": int(output("sysctl", "-n", "hw.memsize")),
                        "os": output("sw_vers", "-productVersion"),
                        "os_build": output("sw_vers", "-buildVersion"),
                        "architecture": platform.machine(),
                        "rustc": output("rustc", "--version"),
                        "profile": "release; thin LTO; codegen-units=1"},
        "dataset": {"name": "BEIR SciFact", "base_sha256": sha256(args.base),
                    "query_sha256": sha256(args.query)},
        "method": {"rounds": 3, "m": 16, "ef_construction": 200, "seed": 42,
                   "warmup_queries": 10, "metric": "cosine", "k": 10,
                   "scope": "native in-memory HNSW search calls including result construction",
                   "exclusions": ["embeddings", "file loading", "graph construction",
                                  "exact oracle", "process startup", "persistence", "SQ8", "SIMD/mmap"],
                   "cache": "warm; no filesystem-cache flush",
                   "concurrency": "one benchmark child at a time; no concurrent search clients",
                   "host_activity": "other host/editor activity uncontrolled"},
        "runs": [],
    }
    settings = [16, 32, 64, 128, 256, 512]
    for round_index in range(3):
        shift = round_index * 2
        for ef in settings[shift:] + settings[:shift]:
            result = json.loads(output(sys.executable, str(ROOT / "tools/measure_vecnook.py"),
                                       str(args.base.resolve()), str(args.query.resolve()),
                                       "--binary", str(binary), "--ef", str(ef)))
            if (result["version"] != version or result["count"] != 5183
                    or result["queries"] != 300 or result["dimensions"] != 768
                    or result["incomplete_queries"] != 0):
                raise ValueError(f"Unexpected benchmark identity/results: {result}")
            result["round"] = round_index + 1
            evidence["runs"].append(result)
            args.output.write_text(json.dumps(evidence, indent=2) + "\n")
            print(f"round={round_index+1} ef={ef} p95_ms={result['p95_ms']} "
                  f"recall={result['recall']*100:.4f}%", flush=True)
    if sha256(binary) != identity:
        raise ValueError("Binary changed during measurement; discard this sweep")
    evidence["summary"] = []
    for ef in settings:
        samples = [r for r in evidence["runs"] if r["ef"] == ef]
        row = {"ef": ef}
        for field in ["recall", "p50_ms", "p95_ms", "sequential_qps", "build_seconds"]:
            row[field] = statistics.median(r[field] for r in samples)
        row["p95_min_ms"] = min(r["p95_ms"] for r in samples)
        row["p95_max_ms"] = max(r["p95_ms"] for r in samples)
        evidence["summary"].append(row)
    evidence["completed_at_utc"] = datetime.now(timezone.utc).isoformat()
    args.output.write_text(json.dumps(evidence, indent=2) + "\n")


if __name__ == "__main__":
    main()
