#!/usr/bin/env python3
"""Combine two completed four-round comparison series without dropping runs."""
import argparse
import copy
import json
from pathlib import Path

from benchmark_before_after import summarize
from benchmark_readme import sha256


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("series", nargs=2, type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error("choose a fresh output path")
    sources = [json.loads(p.read_text()) for p in args.series]
    one, two = sources
    for source in sources:
        if not source.get("completed_at_utc") or len(source["runs"]) != 16 or source["method"]["rounds"] != 4:
            raise ValueError("expected two completed four-round series")
        for name in source["method"]["order"]:
            runs = [r for r in source["runs"] if r["name"] == name]
            if len(runs) != 4 or sorted(r["round"] for r in runs) != [1, 2, 3, 4]:
                raise ValueError("a scheduled result is missing or duplicated")
            if any(r["incomplete_queries"] for r in runs):
                raise ValueError("incomplete results require investigation")
    for key in ["source_revision", "binaries", "environment", "dataset"]:
        if one[key] != two[key]:
            raise ValueError(f"series identity differs: {key}")
    for key in ["ef", "m", "ef_construction", "metric", "dtype", "k", "warmup_queries", "hnsw_threads", "order"]:
        if one["method"][key] != two["method"][key]:
            raise ValueError(f"series configuration differs: {key}")
    result = copy.deepcopy(one)
    result.update(schema=3, completed_at_utc=two["completed_at_utc"], runs=[], series=[])
    result["method"].update(rounds=8, series="two fixed four-round rotations; follow-up after large host variation; all runs included",
                            host_activity=two["method"]["host_activity"])
    for number, (path, source) in enumerate(zip(args.series, sources), 1):
        result["series"].append({"series": number, "raw_file": path.name, "sha256": sha256(path),
                                 "started_at_utc": source["started_at_utc"], "completed_at_utc": source["completed_at_utc"],
                                 "summary": source["summary"]})
        for run in source["runs"]:
            row = copy.deepcopy(run)
            row.update(series=number, series_round=run["round"], round=run["round"] + (number - 1) * 4)
            result["runs"].append(row)
    result["summary"] = summarize(result["runs"], one["method"]["order"])
    before, after = result["summary"][:2]
    result["change"] = {"p95_reduction_percent": 100 * (1 - after["p95_ms"] / before["p95_ms"]),
                         "p50_reduction_percent": 100 * (1 - after["p50_ms"] / before["p50_ms"]),
                         "same_vecnook_recall": before["recall"] == after["recall"]}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(args.output)


if __name__ == "__main__":
    main()
