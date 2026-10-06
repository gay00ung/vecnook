#!/usr/bin/env python3
"""Exercise CLI JSON using independent Python and JavaScript consumers."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "examples/documents"))
from search import Chunk, cli, encode_document


def main():
    binary = Path(os.environ.get("VECNOOK_BINARY", "target/debug/vecnook")).resolve()
    node = shutil.which("node")
    if node is None:
        raise RuntimeError("Node.js is required to verify the JavaScript consumer")
    identifier = str(2**64 - 1)
    original = '한글 🚀\nquotes " and tab\t and backslash\\\r\n'
    with tempfile.TemporaryDirectory(prefix="vecnook-contract-") as temporary:
        root = Path(temporary)
        cli(binary, "init", root / "vectors", 2)
        cli(binary, "put", root / "vectors", identifier, "1,0", original)
        report = json.loads(cli(binary, "search", root / "vectors", "1,0", 1, "--json"))
        assert report["schema_version"] == 1 and report["neighbors"][0]["id"] == identifier
        assert report["neighbors"][0]["metadata"] == original
        space = [root, "documents", 2, "fixture-v1", "cosine"]
        cli(binary, "docs-init", *space)
        batch = root / "input.tsv"
        payload = encode_document(int(identifier), Chunk("한글.md", 1, 3, original), ["rust"])
        batch.write_text(f"put\t{identifier}\t1,0\t{payload}\n", encoding="utf-8")
        cli(binary, "docs-batch", *space, batch)
        document = json.loads(cli(binary, "docs-get", *space, identifier))
        matches = json.loads(cli(binary, "docs-search", *space, "1,0", 1))
        assert document["schema_version"] == 1 and document["id"] == identifier
        assert document["text"] == original and document["source"] == "한글.md"
        assert matches["matches"][0]["document"] == document
        script = """
const assert = require('node:assert/strict');
let input = ''; process.stdin.setEncoding('utf8');
process.stdin.on('data', part => input += part);
process.stdin.on('end', () => {
  const {report, document, matches, original} = JSON.parse(input);
  assert.equal(report.schema_version, 1);
  assert.equal(typeof document.id, 'string');
  assert.equal(BigInt(document.id), (1n << 64n) - 1n);
  assert.equal(report.neighbors[0].id, document.id);
  assert.equal(document.text, original);
  assert.deepEqual(matches.matches[0].document, document);
  console.log('Python/JavaScript JSON v1, full u64 and original UTF-8 passed');
});
"""
        subprocess.run([node, "-e", script], input=json.dumps(dict(report=report, document=document,
                       matches=matches, original=original)), text=True, encoding="utf-8", check=True)


if __name__ == "__main__":
    main()
