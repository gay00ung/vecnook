#!/usr/bin/env python3
"""Allow prereleases; require checked RC assets before workflow stable publication."""
import argparse
from pathlib import Path
import subprocess
import sys
import tomllib
from check_distribution import download


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate-version", default="")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    version = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]
    if "-" in version:
        print("Prerelease package: stable-only duration/Windows 11 gates remain pending until proven.")
        return
    candidate = args.candidate_version
    if not candidate.startswith(version+"-rc.") or not candidate.removeprefix(version+"-rc.").isdigit():
        raise ValueError("stable publication requires the exact checked VERSION-rc.N candidate")
    evidence = root / "work/publication-evidence"
    evidence.mkdir(parents=True, exist_ok=False)
    archive = download(candidate, evidence)
    subprocess.run(["gh", "release", "download", "v"+candidate, "--repo", "gay00ung/vecnook",
                    "--pattern", "distribution-*.json", "--pattern", "soak-receipt.json", "--dir", str(evidence)], check=True)
    receipts = sorted(evidence.glob("distribution-*.json"))
    release = root / "target/package" / f"vecnook-{version}.crate"
    command = [sys.executable, str(root / "tools/check_release.py"), "--candidate-crate", str(archive),
               "--release-crate", str(release), "--soak", str(evidence / "soak-receipt.json")]
    for receipt in receipts:
        command += ["--distribution", str(receipt)]
    subprocess.run(command, check=True)


if __name__ == "__main__":
    main()
