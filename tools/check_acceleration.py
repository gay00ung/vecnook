#!/usr/bin/env python3
"""Install and verify both exact registry packages through the optional adapter."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import tarfile
import tomllib
from urllib.request import Request, urlopen
from check_distribution import digest, fingerprint, windows_info
from check_package import check

ACCEL_CHECKS = {"registry_cli_install", "registry_library_resolution", "simd_bits",
                "mapped_originals", "mapped_process_pin", "external_app_restart",
                "independent_backup", "generation_gc"}


def download(name, version, destination):
    if name not in {"vecnook", "vecnook-accel"} or not re.fullmatch(r"\d+\.\d+\.\d+(?:-[a-zA-Z0-9.-]+)?", version):
        raise ValueError("invalid registry package/version")
    def get(url):
        with urlopen(Request(url, headers={"User-Agent": "vecnook-acceleration-check"}), timeout=120) as response:
            body = response.read(16*1024*1024+1)
        if len(body) > 16*1024*1024:
            raise ValueError("registry response exceeds 16 MiB")
        return body
    entry = json.loads(get(f"https://crates.io/api/v1/crates/{name}/{version}"))["version"]
    body = get(f"https://crates.io/api/v1/crates/{name}/{version}/download")
    if hashlib.sha256(body).hexdigest() != entry["checksum"]:
        raise ValueError("registry checksum mismatch")
    target = destination / f"{name}-{version}.crate"
    target.write_bytes(body)
    return target


def extract(archive, destination, expected_name):
    archive = Path(archive)
    check(archive)
    with tarfile.open(archive) as package:
        prefixes = {member.name.split("/")[0] for member in package.getmembers()}
        if len(prefixes) != 1 or not next(iter(prefixes)).startswith(expected_name+"-"):
            raise ValueError("unexpected package root")
        package.extractall(destination, filter="data")
    source = next(destination.iterdir())
    if tomllib.loads((source / "Cargo.toml").read_text())["package"]["name"] != expected_name:
        raise ValueError("unexpected package identity")
    return source


def acceleration_fingerprint(source, core_version, core_hash):
    """Normalize only paired release identities; verify the actual core checksum first.

    The companion source, tests, examples, dependency options and every other
    lock entry stay bound. Core runtime equivalence is a separate mandatory gate.
    """
    settings = tomllib.loads((source / "Cargo.toml.orig").read_text())
    if settings["package"]["name"] != "vecnook-accel" or settings["dependencies"]["vecnook"]["version"] != "="+core_version:
        raise ValueError("adapter does not declare the exact paired core version")
    settings["package"]["version"] = "RELEASE"
    settings["dependencies"]["vecnook"]["version"] = "=PAIRED_CORE"
    lock = tomllib.loads((source / "Cargo.lock").read_text())
    core = [p for p in lock.get("package", []) if p["name"] == "vecnook"]
    own = [p for p in lock.get("package", []) if p["name"] == "vecnook-accel"]
    if (len(core) != 1 or len(own) != 1 or core[0]["version"] != core_version
            or core[0].get("checksum") != core_hash
            or core[0].get("source") != "registry+https://github.com/rust-lang/crates.io-index"):
        raise ValueError("adapter lock does not bind the independently checked core registry archive")
    core[0].update(version="PAIRED_CORE", checksum="PAIRED_CORE_CHECKSUM")
    own[0]["version"] = "RELEASE"
    value = hashlib.sha256()
    for folder in ["src", "tests", "examples"]:
        for path in sorted((source / folder).rglob("*.rs")):
            value.update(path.relative_to(source).as_posix().encode()+b"\0"+path.read_bytes())
    for data in [settings, lock]:
        value.update(json.dumps(data, sort_keys=True, separators=(",", ":")).encode())
    return value.hexdigest()


def require_registry_lock(lock_path, identities):
    lock = tomllib.loads(Path(lock_path).read_text())
    for name, version, checksum in identities:
        rows = [p for p in lock["package"] if p["name"] == name]
        if (len(rows) != 1 or rows[0]["version"] != version or rows[0].get("checksum") != checksum
                or rows[0].get("source") != "registry+https://github.com/rust-lang/crates.io-index"):
            raise ValueError(f"consumer did not resolve the exact checked {name} registry package")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--registry", required=True, help="exact vecnook-accel version")
    parser.add_argument("--core-registry", required=True, help="exact paired vecnook version")
    parser.add_argument("--output", required=True, help="new directory")
    parser.add_argument("--toolchain", choices=["1.89.0", "stable"], default="1.89.0")
    args = parser.parse_args()
    root = Path(args.output).resolve()
    root.mkdir(parents=True, exist_ok=False)
    core_archive = download("vecnook", args.core_registry, root)
    adapter_archive = download("vecnook-accel", args.registry, root)
    core = extract(core_archive, root / "core-source", "vecnook")
    source = extract(adapter_archive, root / "adapter-source", "vecnook-accel")
    core_hash, adapter_hash = digest(core_archive), digest(adapter_archive)
    source_hash = acceleration_fingerprint(source, args.core_registry, core_hash)
    environment = dict(os.environ, CARGO_HOME=str(root / "cargo-home"))
    cargo = ["cargo", "+"+args.toolchain]
    def command(*parts, cwd=source):
        print("Running: "+" ".join(map(str, parts)), flush=True)
        result = subprocess.run(list(map(str, parts)), cwd=cwd, env=environment,
                                capture_output=True, text=True, encoding="utf-8")
        if result.returncode:
            print(result.stdout, flush=True)
            print(result.stderr, flush=True)
            result.check_returncode()
        return result.stdout
    command(*cargo, "install", "vecnook-accel", "--version", "="+args.registry, "--locked",
            "--root", root / "install", "--target-dir", root / "build")
    identities = [("vecnook", args.core_registry, core_hash), ("vecnook-accel", args.registry, adapter_hash)]
    for name, version, checksum in identities:
        cached = list((root / "cargo-home/registry/cache").glob(f"*/{name}-{version}.crate"))
        if len(cached) != 1 or digest(cached[0]) != checksum:
            raise ValueError("Cargo installation differs from independently checked registry archives")
    binary = root / "install/bin" / ("vecnook-accel.exe" if os.name == "nt" else "vecnook-accel")
    if command(binary, "--version").strip() != "vecnook-accel "+args.registry:
        raise ValueError("installed adapter version differs")
    demo = json.loads(command(binary, "demo", root / "demo"))
    if demo.get("passed") is not True or not {"mmap_originals", "simd_exact_rerank", "pinned_publication", "full_u64", "signed_zero", "independent_backup"}.issubset(demo.get("checks", [])):
        raise ValueError("installed demo did not verify required mapping/original checks")
    search = json.loads(command(binary, "search", root / "demo", "0,0,1", "1", "--sq8", "2", "--mmap"))
    if search.get("mmap") is not True or search["neighbors"][0]["id"] != str(2**64-1) or search["neighbors"][0]["distance"] != 0:
        raise ValueError("installed mapped CLI returned an incorrect original result")
    command(*cargo, "test", "--offline", "--locked", "--manifest-path", source / "Cargo.toml",
            "--target-dir", root / "tests", "--test", "platform")
    consumer = root / "consumer"
    (consumer / "src").mkdir(parents=True)
    (consumer / "Cargo.toml").write_text('[package]\nname="vecnook-acceleration-consumer"\nversion="0.0.0"\nedition="2024"\n[dependencies]\n'
        'vecnook={version='+json.dumps("="+args.core_registry)+'}\nvecnook-accel={version='+json.dumps("="+args.registry)+'}\n')
    shutil.copyfile(source / "examples/consumer.rs", consumer / "src/main.rs")
    for _ in range(2):
        output = command(*cargo, "run", "--offline", "--manifest-path", consumer / "Cargo.toml", "--", root / "application", cwd=consumer)
        if json.loads(output).get("passed") is not True:
            raise ValueError("external library consumer failed")
    require_registry_lock(consumer / "Cargo.lock", identities)
    receipt = dict(schema_version=1, passed=True, registry=True, version=args.registry, core_version=args.core_registry,
                   core_crate_sha256=core_hash, core_source_sha256=fingerprint(core),
                   adapter_crate_sha256=adapter_hash, adapter_source_sha256=source_hash,
                   binary_sha256=digest(binary), toolchain=args.toolchain,
                   compiler=command("rustc", "+"+args.toolchain, "-Vv").strip(),
                   platform=dict(system=platform.system(), release=platform.release(), machine=platform.machine()),
                   windows=windows_info(root), backend=demo["backend"], checks=sorted(ACCEL_CHECKS))
    (root / "receipt.json").write_text(json.dumps(receipt, indent=2)+"\n")
    print("Actual paired registry consumers passed: "+str(root / "receipt.json"))


if __name__ == "__main__":
    main()
