#!/usr/bin/env python3
"""Compare frozen generation readers in fresh processes, including full peak RSS."""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import platform
import re
import statistics
import subprocess
import time


def sha(path):
    value = hashlib.sha256()
    with Path(path).open("rb") as source:
        for part in iter(lambda: source.read(1024 * 1024), b""):
            value.update(part)
    return value.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, help="release engine_bench example")
    parser.add_argument("--base", required=True)
    parser.add_argument("--queries", required=True)
    parser.add_argument("--manifest", help="frozen input provenance JSON; copied without changing it")
    parser.add_argument("--output", required=True, help="new output directory")
    args = parser.parse_args()
    system = platform.system()
    if system not in {"Darwin", "Linux"}:
        parser.error("this RSS collector requires macOS time -l or GNU time -v")
    root = Path(args.output).resolve()
    root.mkdir(parents=True, exist_ok=False)
    binary = Path(args.binary).resolve()
    fixture = root / "fixture"
    started = datetime.datetime.now(datetime.timezone.utc).isoformat()
    record = dict(schema_version=1, completed=False, started_utc=started,
                  platform=platform.platform(), machine=platform.machine(), binary_sha256=sha(binary),
                  base_sha256=sha(args.base), query_sha256=sha(args.queries), repetitions=3,
                  calibration_queries=20, validation_queries=100, recall_target=0.99,
                  selection="first ascending budget with calibration recall@10=1.0; validation is never used for selection",
                  budgets=[10, 16, 32, 64, 128, 256, 512, 1024, 2048, 8192, 65536],
                  conditions="warm filesystem after preparation/calibration; fresh process per run; full peak RSS includes initialization, validation, mapped resident pages and SQ8 build; no overlapping Vecnook builds/models/soaks launched by this measurement; lightweight tooling/editor activity may overlap; other user/system activity uncontrolled")
    if args.manifest:
        record["provenance"] = json.loads(Path(args.manifest).read_text())
    (root / "environment.json").write_text(json.dumps(record, indent=2)+"\n")

    def invoke(parts, name, measured=False):
        command = [str(binary), *map(str, parts)]
        if measured:
            command = ["/usr/bin/time", "-l" if system == "Darwin" else "-v", *command]
        start = time.monotonic()
        result = subprocess.run(command, capture_output=True, text=True, encoding="utf-8", timeout=3600)
        (root / (name+".stdout.log")).write_text(result.stdout)
        (root / (name+".stderr.log")).write_text(result.stderr)
        if result.returncode:
            raise RuntimeError(f"{name} failed; preserved output in {root}")
        row = json.loads(result.stdout.strip().splitlines()[-1])
        row["wall_seconds"] = time.monotonic() - start
        if measured:
            pattern = r"(\d+)\s+maximum resident set size" if system == "Darwin" else r"Maximum resident set size \(kbytes\):\s*(\d+)"
            match = re.search(pattern, result.stderr)
            if not match:
                raise ValueError("time did not report full peak resident set size")
            row["peak_rss_bytes"] = int(match[1]) * (1 if system == "Darwin" else 1024)
        return row

    prepared = invoke(["prepare", fixture, Path(args.base).resolve(), Path(args.queries).resolve()], "prepare")
    record["preparation"] = prepared
    record["oracle_sha256"] = sha(fixture / "oracle.tsv")
    calibrated = []
    selected = {}
    for mode in ["file-sq8"] + (["legacy"] if prepared["legacy_supported"] else []):
        for budget in record["budgets"]:
            if mode == "legacy" and budget > 2048:
                break
            row = invoke(["run", fixture, mode, budget], f"calibrate-{mode}-{budget}")
            calibrated.append(row)
            if row["calibration_recall_at_10"] == 1.0:
                selected[mode] = budget
                break
        if mode not in selected:
            record.update(calibration=calibrated, error=f"no calibration budget reached full recall for {mode}")
            (root / "summary.json").write_text(json.dumps(record, indent=2)+"\n")
            raise RuntimeError(record["error"])
    modes = [("file", 0), ("mmap", 0), ("mmap-simd", 0),
             ("file-sq8", selected["file-sq8"]), ("mmap-sq8", selected["file-sq8"])]
    if prepared["legacy_supported"]:
        modes.append(("legacy", selected["legacy"]))
    runs = []
    for repeat in range(3):
        shift = repeat * 2 % len(modes)
        for mode, budget in modes[shift:] + modes[:shift]:
            row = invoke(["run", fixture, mode, budget], f"run-{repeat+1}-{mode}", measured=True)
            row["repeat"] = repeat + 1
            runs.append(row)
            record.update(calibration=calibrated, selected_budgets=selected, runs=runs)
            (root / "summary.json").write_text(json.dumps(record, indent=2)+"\n")
            print(json.dumps({k: row[k] for k in ["mode", "repeat", "recall_at_10", "p95_ms", "peak_rss_bytes"]}), flush=True)
    summaries = {}
    for mode, _ in modes:
        rows = [row for row in runs if row["mode"] == mode]
        summaries[mode] = {"recall_min": min(row["recall_at_10"] for row in rows),
                           "p95_ms_median": statistics.median(row["p95_ms"] for row in rows),
                           "peak_rss_mib_median": statistics.median(row["peak_rss_bytes"] for row in rows)/1024**2,
                           "peak_rss_mib_samples": [row["peak_rss_bytes"]/1024**2 for row in rows],
                           "code_bytes": rows[0]["code_bytes"], "parameter_bytes": rows[0]["parameter_bytes"]}
    record.update(completed=True, quality_gate=all(row["recall_at_10"] >= 0.99 for row in runs),
                  finished_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(), summaries=summaries)
    (root / "summary.json").write_text(json.dumps(record, indent=2)+"\n")
    if not record["quality_gate"]:
        raise RuntimeError("held-out recall gate failed; preserve results and revise the candidate")


if __name__ == "__main__":
    main()
