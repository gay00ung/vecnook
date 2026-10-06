#!/usr/bin/env python3
"""Prepare a separate pinned USearch Rust workspace and measure sequential fresh processes."""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", required=True)
    parser.add_argument("--query", required=True)
    parser.add_argument("--output", required=True, help="new comparison workspace, outside core package")
    parser.add_argument("--repetitions", type=int, default=3)
    parser.add_argument("--binary", help="already-built comparison binary; skip workspace build")
    args = parser.parse_args()
    if args.repetitions < 3:
        parser.error("at least three fresh processes required")
    root = Path(args.output).resolve()
    root.mkdir(parents=True, exist_ok=False)
    if args.binary:
        binary = Path(args.binary).resolve()
    else:
        workspace = root / "comparison"
        (workspace / "src").mkdir(parents=True)
        shutil.copyfile(ROOT / "tools/compare_usearch.rs", workspace / "src/main.rs")
        manifest = ('[package]\nname="vecnook-usearch-comparison"\nversion="0.0.0"\nedition="2024"\n'
                    '[dependencies]\nusearch="=2.26.4"\nvecnook={path='+json.dumps(str(ROOT))+'}\n'
                    '[profile.release]\nlto="thin"\ncodegen-units=1\n')
        (workspace / "Cargo.toml").write_text(manifest)
        subprocess.run(["cargo", "+stable", "build", "--release", "--manifest-path", str(workspace / "Cargo.toml")], check=True)
        binary = workspace / "target/release/vecnook-usearch-comparison"
    hashes = dict(base=digest(args.base), query=digest(args.query), binary=digest(binary))
    rows = []
    for repeat in range(args.repetitions):
        for engine in ["vecnook", "usearch"]:
            label = f"{engine}-r{repeat+1}"
            stdout, stderr = root / (label+".jsonl"), root / (label+".stderr")
            command = [str(binary), engine, str(Path(args.base).resolve()), str(Path(args.query).resolve())]
            if sys.platform == "darwin":
                command = ["/usr/bin/time", "-l", *command]
            elif sys.platform.startswith("linux"):
                command = ["/usr/bin/time", "-v", *command]
            print(f"Running {label}", flush=True)
            with stdout.open("w") as out, stderr.open("w") as err:
                subprocess.run(command, stdout=out, stderr=err, check=True)
            events = [json.loads(line) for line in stdout.read_text().splitlines()]
            text = stderr.read_text()
            mac = re.search(r"(\d+)\s+maximum resident set size", text)
            linux = re.search(r"Maximum resident set size \(kbytes\): (\d+)", text)
            peak = int(mac[1]) if mac else int(linux[1])*1024 if linux else None
            final = events[-1]
            rows.append(dict(label=label, repeat=repeat+1, peak_rss_bytes=peak, events=events,
                             recall_goal_passed=final["recall_at_10"] >= .99 and final["underfilled"] == 0))
            print(f"Finished {label}: recall={final['recall_at_10']:.4f}, ef={final['ef']}, p95={final['p95_ms']:.4f} ms", flush=True)
    report = dict(platform=platform.platform(), usearch_version="2.26.4", hashes=hashes,
                  repetitions=args.repetitions, runs=rows,
                  rss_scope="Whole Rust child: same input/query arrays and f64 oracle; engine allocations and native runtime differ")
    (root / "results.json").write_text(json.dumps(report, indent=2)+"\n")


if __name__ == "__main__":
    main()
