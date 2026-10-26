#!/usr/bin/env python3
"""Run a separate mapped reader against the exact core package's live publisher."""
import argparse
import datetime
import json
import os
from pathlib import Path
import platform
import subprocess
import time
import tomllib
from check_acceleration import acceleration_fingerprint, extract
from check_distribution import digest, fingerprint


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--core-crate", required=True)
    parser.add_argument("--adapter-crate", required=True)
    parser.add_argument("--publisher-receipt", required=True, help="running core run_soak.py receipt.json")
    parser.add_argument("--output", required=True, help="new directory")
    parser.add_argument("--seconds", type=int, default=86400)
    parser.add_argument("--records", type=int, default=100000)
    parser.add_argument("--toolchain", choices=["1.89.0", "stable"], default="1.89.0")
    args = parser.parse_args()
    if not 0 <= args.seconds <= 7*86400 or args.records < 1:
        parser.error("duration 0..604800 and positive records required")
    root = Path(args.output).resolve()
    root.mkdir(parents=True, exist_ok=False)
    core = extract(args.core_crate, root / "core-source", "vecnook")
    source = extract(args.adapter_crate, root / "adapter-source", "vecnook-accel")
    core_hash, adapter_hash = digest(args.core_crate), digest(args.adapter_crate)
    core_source = fingerprint(core)
    core_version = tomllib.loads((core / "Cargo.toml").read_text())["package"]["version"]
    adapter_source = acceleration_fingerprint(source, core_version, core_hash)
    publisher_receipt = Path(args.publisher_receipt).resolve()
    publisher = json.loads(publisher_receipt.read_text())
    if publisher.get("crate_sha256") != core_hash or publisher.get("source_sha256") != core_source:
        raise ValueError("publisher workload is not bound to the exact checked core archive")
    store = publisher_receipt.parent / "case/generations"
    environment = dict(os.environ, CARGO_HOME=str(root / "cargo-home"))
    subprocess.run(["cargo", "+"+args.toolchain, "build", "--locked", "--release", "--manifest-path", str(source / "Cargo.toml"),
                    "--example", "accel_soak", "--target-dir", str(root / "build")], cwd=source, env=environment, check=True)
    cached = list((root / "cargo-home/registry/cache").glob(f"*/vecnook-{core_version}.crate"))
    if len(cached) != 1 or digest(cached[0]) != core_hash:
        raise ValueError("reader build resolved a different core registry archive")
    binary = root / "build/release/examples" / ("accel_soak.exe" if os.name == "nt" else "accel_soak")
    record = dict(schema_version=1, completed=False, core_crate_sha256=core_hash, core_source_sha256=core_source,
                  adapter_crate_sha256=adapter_hash, adapter_source_sha256=adapter_source, binary_sha256=digest(binary),
                  publisher_started_utc=publisher["started_utc"], publisher_binary_sha256=publisher["binary_sha256"],
                  publisher_receipt=str(publisher_receipt), store=str(store), minimum_seconds=args.seconds,
                  minimum_records=args.records, started_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  platform=platform.platform(), toolchain=args.toolchain)
    receipt = root / "receipt.json"
    receipt.write_text(json.dumps(record, indent=2)+"\n")
    with (root / "stdout.log").open("w") as stdout, (root / "stderr.log").open("w") as stderr:
        result = subprocess.run([str(binary), str(store), str(root / "case"), str(args.seconds), str(args.records)], stdout=stdout, stderr=stderr)
    # Bind the final, immutable publisher receipt, never its running snapshot.
    deadline = time.monotonic()+300
    while result.returncode == 0 and time.monotonic() < deadline:
        try:
            publisher = json.loads(publisher_receipt.read_text())
        except json.JSONDecodeError:
            time.sleep(0.1)
            continue
        if "exit_code" in publisher:
            break
        time.sleep(1)
    progress = root / "case/progress.json"
    record.update(exit_code=result.returncode, finished_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  publisher_receipt_sha256=digest(publisher_receipt),
                  progress=json.loads(progress.read_text()) if progress.exists() else None)
    record["completed"] = (result.returncode == 0 and record["progress"] is not None and record["progress"].get("completed") is True
                           and publisher.get("completed") is True and publisher.get("exit_code") == 0
                           and publisher.get("crate_sha256") == core_hash and publisher.get("source_sha256") == core_source
                           and publisher.get("started_utc") == record["publisher_started_utc"]
                           and publisher.get("binary_sha256") == record["publisher_binary_sha256"])
    receipt.write_text(json.dumps(record, indent=2)+"\n")
    if not record["completed"]:
        raise RuntimeError("mapped reader/publisher failed; preserve original logs and storage")
    print("Mapped reader completed; full duration/record gates remain explicit in "+str(receipt))


if __name__ == "__main__":
    main()
