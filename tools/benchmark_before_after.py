#!/usr/bin/env python3
"""Rotate two frozen Vecnook binaries, USearch and Chroma through four fresh processes each."""
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


def summarize(runs, names):
    rows = []
    for name in names:
        samples = [r for r in runs if r["name"] == name]
        row = {"name": name, "engine": samples[0]["engine"], "version": samples[0]["version"]}
        for field in ["recall", "p50_ms", "p95_ms", "sequential_qps"]:
            row[field] = statistics.median(r[field] for r in samples)
        row.update(p95_min_ms=min(r["p95_ms"] for r in samples),
                   p95_max_ms=max(r["p95_ms"] for r in samples),
                   recall_min=min(r["recall"] for r in samples),
                   recall_max=max(r["recall"] for r in samples))
        rows.append(row)
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("base", type=Path)
    parser.add_argument("query", type=Path)
    parser.add_argument("--truth", type=Path, required=True)
    parser.add_argument("--rust-truth", type=Path, required=True)
    parser.add_argument("--before-binary", type=Path, required=True)
    parser.add_argument("--after-binary", type=Path, required=True)
    parser.add_argument("--before-evidence", type=Path, required=True)
    parser.add_argument("--host-activity", required=True, help="Describe concurrent work, including QA")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error("choose a fresh output path; preserve earlier results")
    if output("git", "diff", "HEAD", "--name-only", "--", "src", "tests", "tools", "Cargo.toml", "Cargo.lock"):
        parser.error("commit the implementation and measurement tools before running")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    for key in ["OMP_NUM_THREADS", "OPENBLAS_NUM_THREADS", "MKL_NUM_THREADS", "NUMEXPR_NUM_THREADS"]:
        os.environ[key] = "1"
    os.environ["ANONYMIZED_TELEMETRY"] = "False"
    binaries = {"Vecnook before": args.before_binary.resolve(), "Vecnook after": args.after_binary.resolve()}
    identities = {name: sha256(binary) for name, binary in binaries.items()}
    previous = json.loads(args.before_evidence.read_text())
    if identities["Vecnook before"] != previous["binary_sha256"]:
        raise ValueError("before binary differs from the retained original comparison")
    if identities["Vecnook before"] == identities["Vecnook after"]:
        raise ValueError("before and after binaries must differ")
    independent = json.loads(args.truth.read_text())
    rust = json.loads(args.rust_truth.read_text())
    if len(independent) != 300 or len(rust) != 300:
        raise ValueError("expected 300 independent oracle queries")
    for exact, reference in zip(rust, independent):
        if len(exact) != 10 or len(reference) != 10:
            raise ValueError("expected exact Top-10")
        if [n["id"] for n in exact] != [n["id"] for n in reference]:
            raise ValueError("Rust and independent oracle IDs differ")
        if any(abs(a["distance"] - b["distance"]) > 1e-12 for a, b in zip(exact, reference)):
            raise ValueError("Rust and independent oracle distances differ")
    dataset = {"name": "BEIR SciFact", "count": 5183, "dimensions": 768, "queries": 300,
               "base_sha256": sha256(args.base), "query_sha256": sha256(args.query),
               "independent_oracle_sha256": sha256(args.truth),
               "rust_exact_sha256": sha256(args.rust_truth), "oracle_verified": True}
    for key in ["base_sha256", "query_sha256", "independent_oracle_sha256"]:
        if dataset[key] != previous["dataset"][key]:
            raise ValueError(f"frozen input differs: {key}")
    revision = output("git", "rev-parse", "HEAD")
    versions = {name: output(str(binary), "--version").removeprefix("vecnook ")
                for name, binary in binaries.items()}
    evidence = {
        "schema": 2, "started_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_revision": revision,
        "binaries": {name: {"sha256": identities[name], "reported_version": versions[name],
                              "origin": "retained original comparison binary" if name.endswith("before")
                              else "local development build; not a published release candidate"}
                     for name in binaries},
        "original_comparison_sha256": sha256(args.before_evidence),
        "original_comparison_source_revision": previous["source_revision"],
        "environment": {"cpu": output("sysctl", "-n", "machdep.cpu.brand_string"),
                        "memory_bytes": int(output("sysctl", "-n", "hw.memsize")),
                        "os": output("sw_vers", "-productVersion"),
                        "os_build": output("sw_vers", "-buildVersion"),
                        "rustc": output("rustc", "+stable", "--version"), "python": sys.version.split()[0],
                        "packages": sorted(f"{d.metadata['Name']}=={d.version}" for d in metadata.distributions())},
        "dataset": dataset,
        "method": {"rounds": 4, "ef": 128, "m": 16, "ef_construction": 200,
                   "metric": "cosine", "dtype": "f32", "k": 10, "warmup_queries": 10,
                   "hnsw_threads": 1, "query_mode": "sequential single-query calls",
                   "selection": "configuration and four-round rotation fixed before scheduled measurement",
                   "order": ["Vecnook before", "Vecnook after", "USearch", "Chroma"],
                   "seed": {"Vecnook": 42, "USearch": "not exposed", "Chroma": "not exposed"},
                   "scope": "warm query-call latency with result construction; API scopes differ",
                   "host_activity": args.host_activity,
                   "exclusions": ["embeddings", "input loading", "graph construction", "process startup",
                                  "network", "concurrent clients", "memory comparison"],
                   "aggregation": "median of process-level p95; observed min/max, no sample pooling"},
        "runs": [],
    }
    base_query = [str(args.base.resolve()), str(args.query.resolve()), "--ef", "128"]
    commands = {name: ["measure_vecnook.py", *base_query, "--binary", str(binary)]
                for name, binary in binaries.items()}
    commands.update({
        "USearch": ["compare_usearch.py", *base_query, "--truth", str(args.truth.resolve())],
        "Chroma": ["compare_chroma.py", *base_query, "--truth", str(args.truth.resolve())],
    })
    names = list(commands)
    def save():
        temporary = args.output.with_suffix(".tmp")
        temporary.write_text(json.dumps(evidence, indent=2) + "\n")
        temporary.replace(args.output)
    save()
    for round_index in range(4):
        for name in names[round_index:] + names[:round_index]:
            command = commands[name]
            child = subprocess.run([sys.executable, str(ROOT / "tools" / command[0]), *command[1:]],
                                   cwd=ROOT, text=True, capture_output=True, timeout=600)
            if child.returncode:
                evidence["failure"] = {"round": round_index + 1, "name": name, "exit": child.returncode,
                                       "stdout": child.stdout, "stderr": child.stderr}
                save()
                raise RuntimeError(f"scheduled child failed: {name}")
            stdout = child.stdout.strip()
            result = json.loads(stdout)
            if any(result[k] != v for k, v in {"count": 5183, "queries": 300, "dimensions": 768,
                                              "k": 10, "ef": 128, "incomplete_queries": 0}.items()):
                raise ValueError(f"unexpected benchmark result: {result}")
            if name in versions and result["version"] != versions[name]:
                raise ValueError("Vecnook binary version changed")
            if name in versions and result["recall"] != previous["summary"][0]["recall"]:
                raise ValueError("Vecnook recall changed; investigate before accepting speed")
            result.update(round=round_index + 1, name=name, child_stdout=stdout, child_stderr=child.stderr)
            evidence["runs"].append(result)
            save()
            print(f"round={round_index+1} engine={name} p95_ms={result['p95_ms']:.6f} "
                  f"recall={result['recall']*100:.4f}%", flush=True)
    if identities != {name: sha256(binary) for name, binary in binaries.items()}:
        raise ValueError("measured binary changed")
    if output("git", "rev-parse", "HEAD") != revision:
        raise ValueError("source revision changed during measurement")
    evidence["summary"] = summarize(evidence["runs"], names)
    evidence["completed_at_utc"] = datetime.now(timezone.utc).isoformat()
    save()


if __name__ == "__main__":
    main()
