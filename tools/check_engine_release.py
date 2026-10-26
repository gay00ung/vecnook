#!/usr/bin/env python3
"""Require paired core/adapter registry, source, and independent full-soak evidence."""
import argparse
import json
import math
from pathlib import Path
import tempfile
import tomllib
from check_acceleration import ACCEL_CHECKS, acceleration_fingerprint, extract
from check_distribution import digest, fingerprint
from check_release import platform_errors, validate


def validate_acceleration(core_hash, core_source, adapter_hash, adapter_source, rows, soak, publisher_hash):
    errors, accepted = [], []
    identity = dict(core_crate_sha256=core_hash, core_source_sha256=core_source,
                    adapter_crate_sha256=adapter_hash, adapter_source_sha256=adapter_source)
    for row in rows:
        if (type(row.get("schema_version")) is not int or row["schema_version"] != 1
                or row.get("passed") is not True or row.get("registry") is not True
                or any(row.get(k) != v for k, v in identity.items())
                or not ACCEL_CHECKS.issubset(row.get("checks", []))
                or row.get("backend") not in {"Scalar", "Neon", "Avx2"}):
            errors.append("adapter distribution receipt does not prove the exact paired registry packages")
        else:
            accepted.append(row)
    errors += platform_errors(accepted)
    progress = soak.get("progress") or {}
    elapsed = progress.get("elapsed_seconds")
    if (type(soak.get("schema_version")) is not int or soak["schema_version"] != 1
            or soak.get("completed") is not True or type(soak.get("exit_code")) is not int or soak["exit_code"] != 0
            or any(soak.get(k) != v for k, v in identity.items())
            or soak.get("publisher_receipt_sha256") != publisher_hash or progress.get("completed") is not True
            or type(elapsed) not in {int, float} or not math.isfinite(elapsed) or elapsed < 86400
            or type(progress.get("checked_records")) is not int or progress["checked_records"] < 100000
            or type(progress.get("mapped_checks")) is not int or progress["mapped_checks"] < 3
            or type(progress.get("generations_observed")) is not int or progress["generations_observed"] < 3
            or progress.get("duration_gate_24h") is not True or progress.get("records_gate_100k") is not True):
        errors.append("completed paired 24-hour/100k-record multiprocess mapped-reader verification is missing")
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate-crate", required=True)
    parser.add_argument("--adapter-crate", required=True)
    parser.add_argument("--release-crate")
    parser.add_argument("--adapter-release-crate")
    parser.add_argument("--distribution", action="append", required=True)
    parser.add_argument("--acceleration-distribution", action="append", required=True)
    parser.add_argument("--soak", required=True)
    parser.add_argument("--acceleration-soak", required=True)
    args = parser.parse_args()
    if bool(args.release_crate) != bool(args.adapter_release_crate):
        parser.error("promotion requires both prepared stable archives")
    core_hash, adapter_hash = digest(args.candidate_crate), digest(args.adapter_crate)
    with tempfile.TemporaryDirectory(prefix="vecnook-engine-gate-") as temporary:
        root = Path(temporary)
        core = extract(args.candidate_crate, root / "core", "vecnook")
        adapter = extract(args.adapter_crate, root / "adapter", "vecnook-accel")
        core_version = tomllib.loads((core / "Cargo.toml").read_text())["package"]["version"]
        core_source = fingerprint(core)
        adapter_source = acceleration_fingerprint(adapter, core_version, core_hash)
        if args.release_crate:
            stable_core = extract(args.release_crate, root / "stable-core", "vecnook")
            stable_adapter = extract(args.adapter_release_crate, root / "stable-adapter", "vecnook-accel")
            stable_version = tomllib.loads((stable_core / "Cargo.toml").read_text())["package"]["version"]
            if fingerprint(stable_core) != core_source:
                raise ValueError("core runtime/tests/build settings changed; validate a new RC")
            if acceleration_fingerprint(stable_adapter, stable_version, digest(args.release_crate)) != adapter_source:
                raise ValueError("adapter runtime/tests/build settings changed; validate a new paired RC")
        errors = validate(core_hash, core_source, [json.loads(Path(p).read_text()) for p in args.distribution],
                          json.loads(Path(args.soak).read_text()))
        errors += validate_acceleration(core_hash, core_source, adapter_hash, adapter_source,
                                       [json.loads(Path(p).read_text()) for p in args.acceleration_distribution],
                                       json.loads(Path(args.acceleration_soak).read_text()), digest(args.soak))
    if errors:
        for error in errors:
            print("Pending: "+error)
        raise SystemExit(1)
    print("Paired engine release gates passed; clean commit CI and public asset verification remain required.")


if __name__ == "__main__":
    main()
