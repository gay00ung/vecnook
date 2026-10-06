#!/usr/bin/env python3
"""Markdown search using Python's standard library, local Ollama and Vecnook."""

import argparse
from dataclasses import dataclass
import json
import math
from pathlib import Path
import subprocess
import sys
import tempfile
from urllib.parse import urlsplit
from urllib.request import Request, build_opener, ProxyHandler

MAX_FILE = 1024 * 1024
MAX_TOTAL = 32 * MAX_FILE
MAX_CHUNKS = 10_000
CHUNK_BYTES = 3000
DEFAULT_MODEL = "embeddinggemma:latest"


@dataclass
class Chunk:
    source: str
    start_line: int
    end_line: int
    text: str


def read_chunks(root):
    """Read bounded UTF-8 Markdown files without following symlinks outside root."""
    root = Path(root).resolve(strict=True)
    if not root.is_dir():
        raise ValueError("document root must be a directory")
    chunks, total, files = [], 0, 0
    for path in sorted(root.rglob("*.md")):
        if path.is_symlink() or not path.resolve().is_relative_to(root):
            raise ValueError(f"symlink document rejected: {path.name}")
        files += 1
        if files > 512 or path.stat().st_size > MAX_FILE:
            raise ValueError("document limit: 512 files, 1 MiB per file")
        raw = path.read_bytes()
        total += len(raw)
        if len(raw) > MAX_FILE or total > MAX_TOTAL:
            raise ValueError("document byte limit exceeded")
        source = path.relative_to(root).as_posix()
        text, start, end = "", 1, 1
        for line_number, line in enumerate(raw.decode("utf-8").splitlines(keepends=True), 1):
            # A long Unicode line is split by UTF-8 bytes without breaking characters.
            pieces, piece, size = [], "", 0
            for character in line:
                count = len(character.encode("utf-8"))
                if size + count > CHUNK_BYTES:
                    pieces.append(piece)
                    piece, size = "", 0
                piece += character
                size += count
            if piece:
                pieces.append(piece)
            for piece in pieces:
                if text and len((text + piece).encode("utf-8")) > CHUNK_BYTES:
                    if text.strip():
                        chunks.append(Chunk(source, start, end, text))
                    text = ""
                if not text:
                    start = line_number
                text += piece
                end = line_number
                if len(chunks) >= MAX_CHUNKS:
                    raise ValueError("document limit: 10,000 chunks")
        if text.strip():
            chunks.append(Chunk(source, start, end, text))
        if len(chunks) > MAX_CHUNKS:
            raise ValueError("document limit: 10,000 chunks")
    if not chunks:
        raise ValueError("no nonempty Markdown documents found")
    return chunks


class Ollama:
    def __init__(self, url="http://127.0.0.1:11434"):
        parsed = urlsplit(url)
        if (parsed.scheme != "http" or parsed.hostname not in {"localhost", "127.0.0.1", "::1"}
                or parsed.username or parsed.password or parsed.path not in {"", "/"}
                or parsed.query or parsed.fragment):
            raise ValueError("Ollama URL must be a local HTTP origin")
        self.url = url.rstrip("/")
        self.opener = build_opener(ProxyHandler({}))

    def request(self, path, body=None):
        data = None if body is None else json.dumps(body).encode("utf-8")
        request = Request(self.url + path, data=data, headers={"Content-Type": "application/json"})
        with self.opener.open(request, timeout=120) as response:
            raw = response.read(16 * MAX_FILE + 1)
        if len(raw) > 16 * MAX_FILE:
            raise ValueError("embedding response exceeds 16 MiB")
        return json.loads(raw)

    def digest(self, model):
        canonical = model if ":" in model else model + ":latest"
        for entry in self.request("/api/tags").get("models", []):
            if entry.get("name") == canonical and isinstance(entry.get("digest"), str):
                return entry["digest"]
        raise ValueError(f"model is not installed; run ollama pull {model}")

    def embed(self, model, inputs):
        data = self.request("/api/embed", {"model": model, "input": inputs, "truncate": False})
        vectors = data.get("embeddings", [])
        if len(vectors) != len(inputs):
            raise ValueError("embedding response count mismatch")
        dimension = len(vectors[0]) if vectors else 0
        if not 1 <= dimension <= 4096:
            raise ValueError("embedding dimension must be 1..4096")
        for vector in vectors:
            if (len(vector) != dimension or any(type(v) not in {int, float} or
                    not math.isfinite(v) or abs(v) > 3.4028234663852886e38 for v in vector)
                    or not any(vector)):
                raise ValueError("embedding must be a finite, nonzero f32 vector")
        return vectors


def document_input(model, chunk):
    if model.split(":")[0] == "embeddinggemma":
        return f"title: {Path(chunk.source).stem} | text: {chunk.text}"
    return chunk.text


def query_input(model, query):
    return f"task: search result | query: {query}" if model.split(":")[0] == "embeddinggemma" else query


def cli(binary, *args):
    result = subprocess.run([str(binary), *map(str, args)], capture_output=True, text=True,
                            encoding="utf-8", timeout=120)
    if result.returncode:
        raise ValueError(result.stderr.strip() or "Vecnook command failed")
    return result.stdout


def coordinates(vector):
    return ",".join(format(value, ".9g") for value in vector)


def index_documents(args, client):
    db = Path(args.db)
    if db.exists():
        raise ValueError("index requires a new database directory; choose a new --db path")
    chunks = read_chunks(args.documents)
    digest = client.digest(args.model)
    # Embed everything before creating storage, so model errors leave no partial database.
    vectors = []
    for offset in range(0, len(chunks), 16):
        vectors.extend(client.embed(args.model, [document_input(args.model, c)
                                                for c in chunks[offset:offset + 16]]))
    if any(len(v) != len(vectors[0]) for v in vectors):
        raise ValueError("model returned inconsistent embedding dimensions")
    if client.digest(args.model) != digest:
        raise ValueError("model changed while indexing; retry with a fixed model")
    cli(args.binary, "init", db, len(vectors[0]), "--metric", "cosine")
    with tempfile.TemporaryDirectory(prefix="vecnook-import-") as temporary:
        batch = Path(temporary) / "chunks.tsv"
        for offset in range(0, len(chunks), 256):
            rows = []
            for index in range(offset, min(offset + 256, len(chunks))):
                payload = json.dumps(vars(chunks[index]), ensure_ascii=False, separators=(",", ":"))
                if len(payload.encode("utf-8")) > 16 * 1024:
                    raise ValueError("chunk payload exceeds the Vecnook metadata limit")
                rows.append(f"put\t{index}\t{coordinates(vectors[index])}\t{payload}\n")
            batch.write_text("".join(rows), encoding="utf-8")
            cli(args.binary, "batch", db, batch)
    cli(args.binary, "checkpoint", db)
    manifest = {"format": 1, "model": args.model, "model_digest": digest,
                "dimensions": len(vectors[0]), "chunks": len(chunks)}
    (db / "documents.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"Indexed {len(chunks)} chunks, {len(vectors[0])} dimensions, model {args.model}")


def search_documents(args, client):
    manifest_path = Path(args.db) / "documents.json"
    if manifest_path.stat().st_size > 16 * 1024:
        raise ValueError("invalid document manifest size")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest.get("format") != 1:
        raise ValueError("unsupported document manifest")
    model = manifest["model"]
    if client.digest(model) != manifest["model_digest"]:
        raise ValueError("embedding model changed; rebuild this document database")
    if not args.query.strip() or len(args.query.encode("utf-8")) > CHUNK_BYTES:
        raise ValueError("query must contain 1..3000 UTF-8 bytes")
    query = client.embed(model, [query_input(model, args.query)])[0]
    if len(query) != manifest["dimensions"]:
        raise ValueError("query embedding dimensions differ from the indexed model")
    result = json.loads(cli(args.binary, "search", args.db, coordinates(query), args.k,
                            128, "auto", "--json"))
    for neighbor in result["neighbors"]:
        payload = json.loads(neighbor.pop("metadata"))
        neighbor.update(payload)
    if args.json:
        print(json.dumps(result, ensure_ascii=False, indent=2))
    else:
        for neighbor in result["neighbors"]:
            print(f"{neighbor['source']}:{neighbor['start_line']}-{neighbor['end_line']} "
                  f"distance={neighbor['distance']:.6f}")
            print(neighbor["text"].strip() + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default="vecnook", help="path to the installed Vecnook CLI")
    parser.add_argument("--ollama-url", default="http://127.0.0.1:11434")
    commands = parser.add_subparsers(dest="command", required=True)
    index = commands.add_parser("index")
    index.add_argument("documents")
    index.add_argument("--db", required=True)
    index.add_argument("--model", default=DEFAULT_MODEL)
    search = commands.add_parser("search")
    search.add_argument("query")
    search.add_argument("--db", required=True)
    search.add_argument("--k", type=int, choices=range(1, 101), default=3, metavar="1..100")
    search.add_argument("--json", action="store_true")
    args = parser.parse_args()
    try:
        client = Ollama(args.ollama_url)
        (index_documents if args.command == "index" else search_documents)(args, client)
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
