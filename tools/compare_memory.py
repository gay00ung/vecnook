#!/usr/bin/env python3
"""Compile one common-API harness against two source trees; compare fresh processes."""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import re
import shutil
import statistics
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", required=True, help="extracted, checksum-verified registry source")
    parser.add_argument("--candidate", default=".")
    parser.add_argument("--output", required=True)
    parser.add_argument("--count", type=int, default=8000)
    parser.add_argument("--repetitions", type=int, default=3)
    parser.add_argument("--toolchain", default="1.89.0")
    args = parser.parse_args()
    if args.count < 1000 or args.repetitions < 3 or sys.platform not in {"darwin", "linux"}:
        parser.error("requires macOS/Linux, at least 1000 records and three repetitions")
    root = Path(args.output).resolve()
    root.mkdir(parents=True, exist_ok=False)
    harness = Path(__file__).with_name("memory_bench.rs").read_bytes()
    reports = []
    for label, source in [("1.1.0", args.baseline), ("candidate", args.candidate)]:
        source = Path(source).resolve()
        project = root / label
        (project / "src").mkdir(parents=True)
        (project / "src/main.rs").write_bytes(harness)
        (project / "Cargo.toml").write_text('[package]\nname="memory-consumer"\nversion="0.0.0"\nedition="2024"\n[dependencies]\nvecnook={path='+json.dumps(str(source))+'}\n')
        with (project / "build.log").open("w") as log:
            subprocess.run(["cargo", "+"+args.toolchain, "build", "--offline", "--release"], cwd=project, stdout=log, stderr=log, check=True)
        binary = project / "target/release/memory-consumer"
        prepared = project / "prepared"
        subprocess.run([str(binary), "prepare", str(prepared), str(args.count)], check=True, stdout=subprocess.DEVNULL)
        for mode in ["checkpoint", "open", "compact", "update", "delete", "search"]:
            for repeat in range(args.repetitions):
                folder = project / f"{mode}-{repeat}"
                shutil.copytree(prepared, folder)
                timer = ["/usr/bin/time", "-l" if sys.platform == "darwin" else "-v"]
                result = subprocess.run([*timer, str(binary), mode, str(folder), str(args.count)], text=True, capture_output=True, check=True)
                (project / f"{mode}-{repeat}.stdout").write_text(result.stdout)
                (project / f"{mode}-{repeat}.stderr").write_text(result.stderr)
                mac = re.search(r"(\d+)\s+maximum resident set size", result.stderr)
                linux = re.search(r"Maximum resident set size \(kbytes\): (\d+)", result.stderr)
                if not (mac or linux):
                    raise ValueError("OS peak RSS missing")
                row = dict(json.loads(result.stdout), version=label, repeat=repeat+1,
                           peak_rss_bytes=int(mac[1]) if mac else int(linux[1])*1024)
                reports.append(row)
                print(f"{label} {mode} r{repeat+1}: {row['phase_ms']:.2f} ms, peak {row['peak_rss_bytes']/1048576:.2f} MiB", flush=True)
                shutil.rmtree(folder)
        shutil.rmtree(prepared)
    digests = {r["ranking_digest"] for r in reports if r["phase"] == "search"}
    if len(digests) != 1:
        raise ValueError("graph ranking changed between baseline and candidate")
    summary = []
    for phase in ["checkpoint", "open", "compact", "update", "delete", "search"]:
        for version in ["1.1.0", "candidate"]:
            rows = [r for r in reports if r["phase"] == phase and r["version"] == version]
            summary.append(dict(phase=phase, version=version, median_ms=statistics.median(r["phase_ms"] for r in rows),
                                median_peak_rss_bytes=statistics.median(r["peak_rss_bytes"] for r in rows),
                                median_p95_ms=statistics.median(r["p95_ms"] for r in rows)))
    (root / "results.json").write_text(json.dumps(dict(completed=True, platform=platform.platform(), toolchain=args.toolchain,
        harness_sha256=hashlib.sha256(harness).hexdigest(), baseline_source=str(Path(args.baseline).resolve()),
        count=args.count, repetitions=args.repetitions, runs=reports, summary=summary), indent=2)+"\n")


if __name__ == "__main__":
    main()
