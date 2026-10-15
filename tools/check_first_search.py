#!/usr/bin/env python3
"""Time a fresh packaged CLI installation, folder sync and verified first query."""
import argparse
import json
from pathlib import Path
import platform
import subprocess
import tarfile
import time

from check_distribution import digest
from check_package import check


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--crate", required=True)
    parser.add_argument("--documents", required=True)
    parser.add_argument("--output", required=True, help="new directory")
    parser.add_argument("--toolchain", default="1.89.0")
    parser.add_argument("--query", default="How do I recover after a write fails?")
    args = parser.parse_args()
    root = Path(args.output).resolve()
    root.mkdir(parents=True, exist_ok=False)
    archive = Path(args.crate).resolve()
    documents = Path(args.documents).resolve(strict=True)
    originals = {p.relative_to(documents).as_posix(): p.read_bytes()
                 for p in documents.rglob("*.md") if p.is_file()}
    check(archive)
    started = time.perf_counter()
    with tarfile.open(archive) as package:
        package.extractall(root / "source", filter="data")
    source = next((root / "source").iterdir())
    with (root / "install.log").open("w") as log:
        subprocess.run(["cargo", "+"+args.toolchain, "install", "--offline", "--locked",
                        "--path", str(source), "--root", str(root / "install"),
                        "--target-dir", str(root / "build")], stdout=log, stderr=log, check=True)
    installed = time.perf_counter()
    binary = root / "install/bin" / ("vecnook.exe" if platform.system() == "Windows" else "vecnook")
    database = root / "documents"
    # Run outside the checkout to catch accidental dependencies on repository paths.
    sync = subprocess.run([str(binary), "docs-sync", str(documents), "--db", str(database)],
                          cwd=root, capture_output=True, text=True, encoding="utf-8", timeout=240, check=True)
    synced = time.perf_counter()
    (root / "sync.log").write_text(sync.stdout + sync.stderr, encoding="utf-8")
    query = subprocess.run([str(binary), "docs-find", args.query, "--db", str(database), "--json"],
                           cwd=root, capture_output=True, text=True, encoding="utf-8", timeout=120, check=True)
    finished = time.perf_counter()
    response = json.loads(query.stdout)
    assert response["neighbors"], "first query returned no original documents"
    for match in response["neighbors"]:
        assert match["source"] in originals
        assert match["text"] in originals[match["source"]].decode("utf-8")
        assert isinstance(match["id"], str) and str(int(match["id"])) == match["id"]
        assert match["start_line"] >= 1 and match["end_line"] >= match["start_line"]
    assert all((documents / path).read_bytes() == body for path, body in originals.items())
    (root / "query.json").write_text(query.stdout, encoding="utf-8")
    record = dict(passed=True, crate_sha256=digest(archive), binary_sha256=digest(binary),
                  toolchain=args.toolchain, markdown_files=len(originals), query=args.query,
                  install_seconds=installed-started, sync_seconds=synced-installed,
                  query_seconds=finished-synced, total_seconds=finished-started,
                  within_five_minutes=finished-started <= 300,
                  ranking=response["retrieval_mode"], returned_sources=[m["source"] for m in response["neighbors"]],
                  scope="Local model, Python and Rust already installed; includes fresh offline candidate CLI installation, sync of actual project Markdown and original-text verification. Model download/toolchain setup excluded.")
    (root / "receipt.json").write_text(json.dumps(record, indent=2)+"\n", encoding="utf-8")
    print(json.dumps(record))
    if not record["within_five_minutes"]:
        raise RuntimeError("first-search target exceeded five minutes; inspect timings")


if __name__ == "__main__":
    main()
