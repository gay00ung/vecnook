#!/usr/bin/env python3
"""Allow prereleases; require checked RC assets before workflow stable publication."""
import argparse
from pathlib import Path
import subprocess
import sys
import tomllib
from check_distribution import download
from check_acceleration import download as download_pair
from prepare_adapter_release import stage


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate-version", default="")
    parser.add_argument("--adapter-candidate-version", default="")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    version = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]
    if "-" in version:
        print("Prerelease package: stable-only duration/Windows 11 gates remain pending until proven.")
        return
    candidate = args.candidate_version
    if not candidate.startswith(version+"-rc.") or not candidate.removeprefix(version+"-rc.").isdigit():
        raise ValueError("stable publication requires the exact checked VERSION-rc.N candidate")
    companion = root / "extensions/accel/Cargo.toml"
    adapter_version = tomllib.loads(companion.read_text())["package"]["version"] if companion.exists() else None
    adapter_candidate = args.adapter_candidate_version
    if adapter_version and (not adapter_candidate.startswith(adapter_version+"-rc.")
                            or not adapter_candidate.removeprefix(adapter_version+"-rc.").isdigit()):
        raise ValueError("stable engine publication requires the paired checked adapter VERSION-rc.N")
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
    if adapter_version:
        adapter_archive = download_pair("vecnook-accel", adapter_candidate, evidence)
        subprocess.run(["gh", "release", "download", "v"+candidate, "--repo", "gay00ung/vecnook",
                        "--pattern", "acceleration-distribution-*.json", "--pattern", "acceleration-soak-receipt.json",
                        "--dir", str(evidence)], check=True)
        prepared = stage(archive, adapter_archive, release, adapter_version, evidence / "staged-adapter.crate")
        paired = [sys.executable, str(root / "tools/check_engine_release.py"), "--candidate-crate", str(archive),
                  "--adapter-crate", str(adapter_archive), "--release-crate", str(release),
                  "--adapter-release-crate", str(prepared), "--soak", str(evidence / "soak-receipt.json"),
                  "--acceleration-soak", str(evidence / "acceleration-soak-receipt.json")]
        for receipt in receipts:
            paired += ["--distribution", str(receipt)]
        for receipt in sorted(evidence.glob("acceleration-distribution-*.json")):
            paired += ["--acceleration-distribution", str(receipt)]
        subprocess.run(paired, check=True)


if __name__ == "__main__":
    main()
