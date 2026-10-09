#!/usr/bin/env python3
"""Run operating-range cases in fresh sequential processes; preserve raw outputs."""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import re
import subprocess
import sys


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default="target/release/examples/capacity_bench")
    parser.add_argument("--output", required=True)
    parser.add_argument("--repetitions", type=int, default=3)
    parser.add_argument("--case", action="append", help="raw:10000:768 or docs:1000:768; default full matrix")
    args = parser.parse_args()
    if args.repetitions < 3:
        parser.error("at least three fresh processes are required")
    root = Path(args.output).resolve()
    root.mkdir(parents=True, exist_ok=False)
    cases = args.case or ([f"raw:{n}:{d}" for n in [1000, 10000, 50000] for d in [128, 512, 768]]
                         + [f"raw:{n}:1536" for n in [1000, 10000]]
                         + [f"docs:{n}:768" for n in [1000, 10000]])
    reports = []
    binary_hash = digest(Path(args.binary).resolve())
    for case in cases:
        mode, count, dimensions = case.split(":")
        hashes = None
        for repeat in range(args.repetitions):
            label = f"{mode}-{count}-{dimensions}-r{repeat+1}"
            folder = root / label
            stdout, stderr = root / (label + ".jsonl"), root / (label + ".stderr")
            command = [str(Path(args.binary).resolve()), mode, count, dimensions, str(folder)]
            if sys.platform == "darwin":
                command = ["/usr/bin/time", "-l", *command]
            elif sys.platform.startswith("linux"):
                command = ["/usr/bin/time", "-v", *command]
            print(f"Running {label}", flush=True)
            with stdout.open("w") as out, stderr.open("w") as err:
                result = subprocess.run(command, stdout=out, stderr=err)
            if result.returncode:
                raise RuntimeError(f"{label} failed; inspect {stderr.name}")
            events = [json.loads(line) for line in stdout.read_text().splitlines()]
            current_hashes = dict(base=digest(folder / "base.fvecs"), query=digest(folder / "query.fvecs"))
            if hashes is not None and hashes != current_hashes:
                raise RuntimeError("fresh processes received different vector/query data")
            hashes = current_hashes
            text = stderr.read_text()
            mac = re.search(r"(\d+)\s+maximum resident set size", text)
            linux = re.search(r"Maximum resident set size \(kbytes\): (\d+)", text)
            peak = int(mac[1]) if mac else int(linux[1])*1024 if linux else None
            qualities = [e for e in events if e["phase"] == "quality"]
            initial = [e for e in qualities if e["label"] == "initial"][-1]
            churn = [e for e in qualities if e["label"] == "after_churn"][-1]
            final_filters = {(e["label"], e["filter"]):e for e in qualities if e["label"] in {"filtered", "filtered_after_churn"}}
            selected = [initial, churn, *final_filters.values()]
            exact = [e for e in qualities if e["label"] == "exact_reference"]
            if len(exact) != 1 or exact[0]["recall_at_10"] != 1 or any(e["underfilled"] for e in selected):
                raise RuntimeError(f"{label} failed exact/underfill validation")
            row = dict(label=label, mode=mode, count=int(count), dimensions=int(dimensions),
                       repeat=repeat+1, hashes=hashes, peak_rss_bytes=peak, events=events,
                       initial_recall_goal_passed=initial["recall_at_10"] >= .99,
                       all_recall_goals_passed=all(e["recall_at_10"] is None or e["recall_at_10"] >= .99 for e in selected))
            reports.append(row)
            (root / "results.json").write_text(json.dumps(dict(platform=platform.platform(), binary_sha256=binary_hash,
                completed=False, repetitions=args.repetitions, runs=reports), indent=2))
            print(f"Finished {label}: recall={initial['recall_at_10']:.4f}, ef={initial['ef']}, p95={initial['p95_ms']:.4f} ms", flush=True)
    (root / "results.json").write_text(json.dumps(dict(platform=platform.platform(), binary_sha256=binary_hash,
        completed=True, repetitions=args.repetitions, runs=reports), indent=2))


if __name__ == "__main__":
    main()
