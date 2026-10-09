#!/usr/bin/env python3
"""Run a complete local Markdown app flow on owned copies of the MIT samples."""
import argparse
from contextlib import redirect_stdout
import io
import json
from pathlib import Path
import shutil
from types import SimpleNamespace
from search import Ollama, cli, index_documents, sync_documents, search_documents


def run(binary, root, client, model="embeddinggemma:latest"):
    root = Path(root).resolve()
    root.mkdir(parents=True, exist_ok=False)
    documents = root / "markdown"
    shutil.copytree(Path(__file__).parent / "sample", documents)
    args = SimpleNamespace(binary=binary, db=root / "notes", documents=documents, model=model,
                           namespace="workflow", tags=["guide"], prune=False)
    index_documents(args, client)
    sync_documents(args, client)  # unchanged: no embedding requests
    source = documents / "recovery.md"
    source.write_bytes(source.read_bytes()+b"\nThis workflow update is stored before backup and restore.\n")
    sync_documents(args, client)
    args.query, args.k, args.json, args.source = "How do I recover my records?", 3, True, "recovery.md"
    def search():
        output = io.StringIO()
        with redirect_stdout(output):
            search_documents(args, client)
        result = json.loads(output.getvalue())
        assert result["neighbors"] and all(n["source"] == "recovery.md" for n in result["neighbors"])
        assert "This workflow update" in "".join(n["text"] for n in result["neighbors"])
        return result
    original = search()
    info = json.loads(cli(binary, "docs-info", root, "notes"))
    space = [root, "notes", info["dimensions"], info["model"], "cosine"]
    # Every standalone CLI call closes and reopens its persistent handle.
    assert search()["neighbors"] == original["neighbors"]
    cli(binary, "docs-backup", *space, root, "backup")
    archive = root / "documents.export"
    cli(binary, "docs-export", *space, archive)
    cli(binary, "docs-import", archive, root, "restored")
    for name in ["backup", "restored"]:
        args.db = root / name
        assert search()["neighbors"] == original["neighbors"]
    print("Import, unchanged sync, live update, source/tag search, restart, backup and independent restore passed.")
    return original


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("new_root")
    parser.add_argument("--binary", default="vecnook")
    parser.add_argument("--model", default="embeddinggemma:latest")
    args = parser.parse_args()
    result = run(args.binary, args.new_root, Ollama(), args.model)
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
