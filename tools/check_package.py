#!/usr/bin/env python3
"""Verify that Cargo archives contain only intended public source files."""
import argparse
from pathlib import Path, PurePosixPath
import tarfile

ROOT_FILES = {".cargo_vcs_info.json", "Cargo.toml", "Cargo.toml.orig", "Cargo.lock",
              "README.md", "LICENSE", "CHANGELOG.md", "CONTRIBUTING.md"}


def check(path):
    with tarfile.open(path) as archive:
        members = archive.getmembers()
        for member in members:
            parts = PurePosixPath(member.name).parts
            if len(parts) < 2 or not parts[0].startswith("vecnook-") or not member.isfile():
                raise ValueError(f"unexpected archive entry: {member.name}")
            relative = PurePosixPath(*parts[1:])
            name = relative.name
            if any(part in {"work", "data", "target", "__pycache__"} for part in relative.parts):
                raise ValueError(f"private/generated path in package: {relative}")
            if name.startswith("LOCAL_") or name in {"PLAN.md", "PLANNING_QA.md", "IMPLEMENTATION_QA.md"}:
                raise ValueError(f"planning document in package: {relative}")
            allowed = len(relative.parts) == 1 and name in ROOT_FILES
            if len(relative.parts) > 1:
                top = relative.parts[0]
                allowed = ((top in {"src", "tests"} and relative.suffix == ".rs")
                           or (top == "docs" and len(relative.parts) == 2 and (relative.suffix == ".md" or name == "demo-provenance.json"))
                           or (top == "examples" and relative.suffix in {".rs", ".md", ".py"})
                           or (top == "tools" and len(relative.parts) == 2 and relative.suffix in {".rs", ".py", ".txt"}))
                allowed |= relative.as_posix() == "docs/media/offline-demo.mp4" and member.size <= 1024 * 1024
            if not allowed:
                raise ValueError(f"unexpected public-source path: {relative}")
    print(f"Verified {path.name}: {len(members)} public source files")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archives", nargs="*")
    parser.add_argument("--prepare-decoys", action="store_true")
    args = parser.parse_args()
    if args.prepare_decoys:
        root = Path("work/package-privacy-decoys")
        root.mkdir(parents=True, exist_ok=True)
        for name in ["LICENSE", "README.md", "LOCAL_PLAN.md", "example.env", "private.fvecs"]:
            (root / name).write_text("package privacy test decoy; never publish\n")
        print("Prepared ignored decoys for package exclusion QA")
        return 0
    paths = [Path(p) for p in args.archives] or sorted(Path("target/package").glob("vecnook-*.crate"))
    if not paths:
        parser.error("no source archive found; run cargo package first")
    for path in paths:
        check(path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
