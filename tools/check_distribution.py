#!/usr/bin/env python3
"""Install the actual source archive and test consumers outside the checkout."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tarfile
import tomllib
from urllib.request import Request, urlopen
import re
from check_package import check


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def fingerprint(source):
    """Bind runtime/source tests and build settings; allow release version/docs changes."""
    value = hashlib.sha256()
    files = sorted(p for folder in ["src", "examples", "tests", "tools"]
                   for p in (source / folder).rglob("*") if p.suffix in {".rs", ".py", ".txt"}
                   and p.is_file())
    files += sorted((source / "examples/documents/sample").glob("*.md"))
    files += [source / "docs/demo-provenance.json"]
    for p in files:
        value.update(p.relative_to(source).as_posix().encode()+b"\0"+p.read_bytes())
    # Cargo's original manifest is unaffected by automatic package normalization.
    manifest = source / "Cargo.toml.orig"
    if not manifest.exists():
        manifest = source / "Cargo.toml"
    settings = tomllib.loads(manifest.read_text())
    settings["package"]["version"] = "RELEASE"
    value.update(json.dumps(settings, sort_keys=True, separators=(",", ":")).encode())
    lock = tomllib.loads((source / "Cargo.lock").read_text())
    for package in lock.get("package", []):
        if package["name"] == "vecnook":
            package["version"] = "RELEASE"
    value.update(json.dumps(lock, sort_keys=True, separators=(",", ":")).encode())
    return value.hexdigest()


def windows_info(root):
    if platform.system() != "Windows":
        return None
    script = ("$os=Get-CimInstance Win32_OperatingSystem; "
              "$vol=Get-Volume -DriveLetter (Get-Location).Drive.Name; "
              "[pscustomobject]@{caption=$os.Caption;product_type=$os.ProductType;build=$os.BuildNumber;"
              "filesystem=$vol.FileSystem}|ConvertTo-Json -Compress")
    result = subprocess.run(["powershell", "-NoProfile", "-Command", script], cwd=root, check=True, capture_output=True, text=True)
    return json.loads(result.stdout)


def download(version, root):
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[a-zA-Z0-9.-]+)?", version):
        raise ValueError("invalid registry version")
    def get(url):
        with urlopen(Request(url, headers={"User-Agent": "vecnook-distribution-check"}), timeout=120) as response:
            body = response.read(16 * 1024 * 1024 + 1)
        if len(body) > 16 * 1024 * 1024:
            raise ValueError("registry response exceeds 16 MiB")
        return body
    entry = json.loads(get(f"https://crates.io/api/v1/crates/vecnook/{version}"))["version"]
    body = get(f"https://crates.io/api/v1/crates/vecnook/{version}/download")
    if hashlib.sha256(body).hexdigest() != entry["checksum"]:
        raise ValueError("registry checksum mismatch")
    path = root / f"vecnook-{version}.crate"
    path.write_bytes(body)
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--crate")
    group.add_argument("--registry")
    parser.add_argument("--output", required=True, help="new directory")
    parser.add_argument("--toolchain", default="1.89.0")
    args = parser.parse_args()
    root = Path(args.output).resolve()
    root.mkdir(parents=True, exist_ok=False)
    archive = Path(args.crate).resolve() if args.crate else download(args.registry, root)
    check(archive)
    with tarfile.open(archive) as package:
        package.extractall(root / "source", filter="data")
    source = next((root / "source").iterdir())
    def command(*parts, cwd=source, env=None):
        print("Running: "+" ".join(map(str, parts)), flush=True)
        subprocess.run(list(map(str, parts)), cwd=cwd, env=env, check=True)
    cargo = ["cargo", "+"+args.toolchain]
    command(*cargo, "install", "--offline", "--locked", "--path", source,
            "--root", root / "install", "--target-dir", root / "build")
    binary = root / "install/bin" / ("vecnook.exe" if os.name == "nt" else "vecnook")
    version = subprocess.run([binary, "--version"], check=True, capture_output=True, text=True).stdout.strip().split()[-1]
    if args.registry and version != args.registry:
        raise ValueError("installed version differs from downloaded registry version")
    env = dict(os.environ, VECNOOK_BINARY=str(binary))
    command(sys.executable, source / "tools/check_contracts.py", env=env)
    command(sys.executable, "-m", "unittest", "discover", "-s", source / "examples/documents", "-p", "test_*.py", env=env)
    command(*cargo, "test", "--offline", "--locked", "--manifest-path", source / "Cargo.toml",
            "--target-dir", root / "tests", "--test", "upgrade", "--test", "operations")
    consumer = root / "consumer"
    (consumer / "src").mkdir(parents=True)
    (consumer / "Cargo.toml").write_text('[package]\nname="vecnook-package-consumer"\nversion="0.0.0"\nedition="2024"\n'
        '[dependencies]\nvecnook={path='+json.dumps(str(source))+'}\n')
    shutil.copyfile(source / "examples/local_app.rs", consumer / "src/main.rs")
    for _ in range(2):
        command(*cargo, "run", "--offline", "--manifest-path", consumer / "Cargo.toml", "--", root / "application", cwd=consumer)
    for _ in range(2):
        command(binary, "demo", root / "offline-demo")
    receipt = dict(schema_version=1, passed=True, version=version, registry=bool(args.registry),
                   crate_sha256=digest(archive), source_sha256=fingerprint(source),
                   binary_sha256=digest(binary), toolchain=args.toolchain,
                   platform=dict(system=platform.system(), release=platform.release(), machine=platform.machine()),
                   windows=windows_info(root), checks=["installed_cli", "json_contracts", "document_flow",
                        "legacy_upgrade", "doctor_export_import_backup", "external_app_restart", "offline_demo_restart"])
    (root / "receipt.json").write_text(json.dumps(receipt, indent=2)+"\n")
    print("Distribution checks passed: "+str(root / "receipt.json"))


if __name__ == "__main__":
    main()
