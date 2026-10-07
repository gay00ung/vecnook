#!/usr/bin/env python3
"""Refuse stable publication without package-bound installation and long-soak evidence."""
import argparse
import json
import math
from pathlib import Path
import tarfile
import tempfile
from check_distribution import digest, fingerprint
from check_package import check

CHECKS = {"installed_cli", "json_contracts", "document_flow", "legacy_upgrade",
          "doctor_export_import_backup", "external_app_restart", "offline_demo_restart",
          "registry_cli_install", "registry_library_resolution"}


def validate(crate_hash, source_hash, distributions, soak):
    errors, pairs, windows11 = [], set(), False
    for row in distributions:
        if (type(row.get("schema_version")) is not int or row["schema_version"] != 1 or row.get("passed") is not True
                or row.get("registry") is not True or row.get("crate_sha256") != crate_hash
                or row.get("source_sha256") != source_hash or not CHECKS.issubset(row.get("checks", []))):
            errors.append("distribution receipt does not prove the candidate registry package")
            continue
        system = row.get("platform", {}).get("system")
        pairs.add((system, row.get("toolchain")))
        windows = row.get("windows") or {}
        windows11 |= (system == "Windows" and type(windows.get("product_type")) is int and windows["product_type"] == 1
                      and "Windows 11" in windows.get("caption", "") and windows.get("filesystem") == "NTFS")
    expected = {(system, rust) for system in ["Linux", "Darwin", "Windows"] for rust in ["1.89.0", "stable"]}
    if not expected.issubset(pairs):
        errors.append("registry package requires successful MSRV/stable distribution checks on all three systems")
    if not windows11:
        errors.append("actual Windows 11 NTFS application/restore receipt is missing (Server CI does not substitute)")
    progress = soak.get("progress") or {}
    elapsed = progress.get("elapsed_seconds")
    mutations = progress.get("ack_mutations")
    if (type(soak.get("schema_version")) is not int or soak["schema_version"] != 1 or soak.get("completed") is not True
            or type(soak.get("exit_code")) is not int or soak["exit_code"] != 0 or soak.get("crate_sha256") != crate_hash
            or soak.get("source_sha256") != source_hash or progress.get("completed") is not True
            or type(elapsed) not in {int, float} or not math.isfinite(elapsed) or elapsed < 86400
            or type(mutations) is not int or mutations < 100000
            or progress.get("duration_gate_24h") is not True or progress.get("mutation_gate_100k") is not True
            or type(progress.get("restarts")) is not int or progress["restarts"] < 3
            or type(progress.get("backups_verified")) is not int or progress["backups_verified"] < 1):
        errors.append("completed candidate-bound 24-hour/100k-mutation/restart/backup ACK verification is missing")
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate-crate", required=True)
    parser.add_argument("--release-crate", help="also require identical runtime/test/build settings, permitting release version/docs changes")
    parser.add_argument("--distribution", action="append", required=True, help="receipt.json; repeat for each OS/toolchain")
    parser.add_argument("--soak", required=True)
    args = parser.parse_args()
    archive = Path(args.candidate_crate)
    check(archive)
    with tempfile.TemporaryDirectory(prefix="vecnook-release-check-") as temporary:
        root = Path(temporary)
        with tarfile.open(archive) as package:
            package.extractall(root / "candidate", filter="data")
        candidate = next((root / "candidate").iterdir())
        source_hash = fingerprint(candidate)
        if args.release_crate:
            check(Path(args.release_crate))
            with tarfile.open(args.release_crate) as package:
                package.extractall(root / "release", filter="data")
            if fingerprint(next((root / "release").iterdir())) != source_hash:
                raise ValueError("runtime/tests/build settings changed since candidate; create and validate a new RC")
        errors = validate(digest(archive), source_hash, [json.loads(Path(p).read_text()) for p in args.distribution],
                          json.loads(Path(args.soak).read_text()))
    if errors:
        for error in errors:
            print("Pending: "+error)
        raise SystemExit(1)
    print("Candidate-bound technical release gates passed. Inspect clean commit CI, version and changelog before publication.")


if __name__ == "__main__":
    main()
