#!/usr/bin/env python3
"""Build and run the mixed workload from an exact checked crate; bind its receipt."""
import argparse
import datetime
import json
from pathlib import Path
import platform
import subprocess
import tarfile
from check_distribution import digest, fingerprint
from check_package import check


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--crate", required=True)
    parser.add_argument("--output", required=True, help="new directory")
    parser.add_argument("--seconds", type=int, default=86400)
    parser.add_argument("--mutations", type=int, default=100000)
    parser.add_argument("--toolchain", default="1.89.0")
    args = parser.parse_args()
    if not 0 <= args.seconds <= 7*86400 or args.mutations < 1:
        parser.error("duration 0..604800 and positive mutations required")
    archive = Path(args.crate).resolve()
    check(archive)
    root = Path(args.output).resolve()
    root.mkdir(parents=True, exist_ok=False)
    with tarfile.open(archive) as package:
        package.extractall(root / "source", filter="data")
    source = next((root / "source").iterdir())
    subprocess.run(["cargo", "+"+args.toolchain, "build", "--release", "--offline", "--locked",
                    "--manifest-path", str(source / "Cargo.toml"), "--example", "soak",
                    "--target-dir", str(root / "build")], check=True)
    binary = root / "build/release/examples" / ("soak.exe" if platform.system() == "Windows" else "soak")
    record = dict(schema_version=1, completed=False, crate_sha256=digest(archive),
                  source_sha256=fingerprint(source), binary_sha256=digest(binary),
                  started_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  minimum_seconds=args.seconds, minimum_mutations=args.mutations,
                  platform=platform.platform(), toolchain=args.toolchain)
    receipt = root / "receipt.json"
    receipt.write_text(json.dumps(record, indent=2)+"\n")
    print("Running checked package workload; actual progress: "+str(root / "case/progress.json"), flush=True)
    with (root / "stdout.log").open("w") as out, (root / "stderr.log").open("w") as err:
        result = subprocess.run([str(binary), str(root / "case"), str(args.seconds), str(args.mutations)], stdout=out, stderr=err)
    progress = root / "case/progress.json"
    record.update(exit_code=result.returncode, finished_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  progress=json.loads(progress.read_text()) if progress.exists() else None)
    record["completed"] = result.returncode == 0 and record["progress"] is not None and record["progress"]["completed"] is True
    receipt.write_text(json.dumps(record, indent=2)+"\n")
    if not record["completed"]:
        raise RuntimeError("workload failed; inspect preserved logs and storage")
    print("Workload completed; duration/mutation release gates remain explicit in "+str(receipt))


if __name__ == "__main__":
    main()
