#!/usr/bin/env python3
"""Markdown search using Python's standard library, local Ollama and Vecnook."""

import argparse
from dataclasses import dataclass
import json
import hashlib
import os
import math
from pathlib import Path
import struct
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


def read_chunks(root, *, allow_empty=False, include_sources=False):
    """Read bounded UTF-8 Markdown files without following symlinks outside root."""
    root = Path(root).resolve(strict=True)
    if not root.is_dir():
        raise ValueError("document root must be a directory")
    chunks, total, files = [], 0, 0
    sources = []
    paths, stack, directories = [], [root], 0
    while stack:
        directory = stack.pop()
        directories += 1
        if directories > 4096:
            raise ValueError("document limit: 4096 directories")
        # scandir reports access errors; rglob can silently suppress them.
        with os.scandir(directory) as entries:
            for entry in entries:
                path = Path(entry.path)
                if entry.is_symlink():
                    raise ValueError(f"symlink document/directory rejected: {path.name}")
                if entry.is_dir(follow_symlinks=False):
                    stack.append(path)
                elif path.suffix == ".md":
                    if not entry.is_file(follow_symlinks=False):
                        raise ValueError("document is not a regular file")
                    paths.append(path)
                    if len(paths) > 512:
                        raise ValueError("document limit: 512 files")
    for path in sorted(paths):
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
        sources.append(source)
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
    if not chunks and not allow_empty:
        raise ValueError("no nonempty Markdown documents found")
    return (chunks, sources) if include_sources else chunks


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


def encode_document(identifier, chunk, tags):
    if not 0 <= identifier <= 2**64 - 1 or len(tags) > 32 or len(set(tags)) != len(tags):
        raise ValueError("document ID/tag limits exceeded")
    data = struct.pack("<QII", identifier, chunk.start_line, chunk.end_line)
    def string(value, limit):
        raw = value.encode("utf-8")
        if len(raw) > limit:
            raise ValueError("document field byte limit exceeded")
        return struct.pack("<I", len(raw)) + raw
    data += string(chunk.source, 1024) + string(chunk.text, 6000)
    data += struct.pack("<I", len(tags))
    for tag in tags:
        if not tag or "\0" in tag:
            raise ValueError("document tags must be nonempty and contain no NUL")
        data += string(tag, 128)
    payload = "VDOC1:" + data.hex()
    if len(payload) > 16 * 1024:
        raise ValueError("encoded document payload exceeds 16 KiB")
    return payload


SYNC_PREFIX = "vecnook:sync:v1:"


def managed_tag(args):
    namespace = getattr(args, "namespace", None)
    if namespace is None:
        namespace = hashlib.sha256(str(Path(args.documents).resolve(strict=True)).encode()).hexdigest()
    if not namespace or len(namespace) > 64 or any(c not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-" for c in namespace):
        raise ValueError("sync namespace must contain 1..64 ASCII letters/digits/_/-")
    return SYNC_PREFIX + namespace


def import_tags(args):
    tags = list(getattr(args, "tags", []))
    if len(tags) > 31 or any(t.startswith(SYNC_PREFIX) for t in tags):
        raise ValueError("at most 31 user tags; the sync tag prefix is reserved")
    return tags + [managed_tag(args)]


def index_documents(args, client):
    db = Path(args.db)
    if db.exists():
        raise ValueError("index requires a new database directory; choose a new --db path")
    chunks = read_chunks(args.documents)
    tags = import_tags(args)
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
    identity = json.dumps({"provider": "ollama", "model": args.model, "digest": digest,
                           "prompt_version": 1}, separators=(",", ":"))
    space = [db.parent, db.name, len(vectors[0]), identity, "cosine"]
    # Prepare and bound complete source batches before creating the database.
    with tempfile.TemporaryDirectory(prefix="vecnook-import-") as temporary:
        grouped = {}
        for index, (chunk, vector) in enumerate(zip(chunks, vectors)):
            payload = encode_document(index, chunk, tags)
            rows, size = grouped.setdefault(chunk.source, ([], [13]))
            rows.append(f"put\t{index}\t{coordinates(vector)}\t{payload}\n")
            size[0] += 13 + len(vector) * 4 + len(payload.encode())
        prepared = []
        for number, (source, (rows, size)) in enumerate(grouped.items()):
            body = "".join(rows)
            if len(rows) > 1024 or size[0] > 8 * MAX_FILE or len(body.encode()) > 16 * MAX_FILE:
                raise ValueError(f"source exceeds atomic batch limits: {source}")
            batch = Path(temporary) / f"source-{number}.tsv"
            batch.write_text(body, encoding="utf-8")
            prepared.append(batch)
        cli(args.binary, "docs-init", *space)
        for sequence, batch in enumerate(prepared):
            cli(args.binary, "docs-batch", *space, batch, "--if-sequence", sequence)
    cli(args.binary, "docs-checkpoint", *space)
    print(f"Indexed {len(chunks)} chunks, {len(vectors[0])} dimensions, model {args.model}")
    return len(grouped)


def collection_state(args, manifest):
    space = [Path(args.db).parent, Path(args.db).name, manifest["dimensions"], manifest["model"], "cosine"]
    documents, after, sequence = [], "-", None
    while True:
        page = json.loads(cli(args.binary, "docs-list", *space, after, 128))
        current = int(page["sequence"])
        if sequence is not None and sequence != current:
            raise ValueError("collection changed while listing; rerun sync")
        sequence = current
        documents.extend(page["documents"])
        if len(documents) > MAX_CHUNKS:
            raise ValueError("sync supports at most 10,000 stored chunks")
        if page["next_after"] is None:
            return documents, sequence, space
        after = page["next_after"]


def sync_documents(args, client):
    # A full, successful scan precedes any writes or pruning, including empty files.
    chunks, sources = read_chunks(args.documents, allow_empty=True, include_sources=True)
    tag, tags = managed_tag(args), import_tags(args)
    db = Path(args.db)
    if not db.exists():
        if not getattr(args, "model", None):
            args.model = DEFAULT_MODEL
        if not chunks:
            raise ValueError("no nonempty Markdown documents found")
        changed = index_documents(args, client)
        print(f"Sync complete: changed={changed} unchanged=0 deleted=0 failed=0")
        return
    manifest = json.loads(cli(args.binary, "docs-info", db.parent, db.name))
    identity = json.loads(manifest["model"])
    if identity.get("provider") != "ollama" or identity.get("prompt_version") != 1:
        raise ValueError("unsupported document embedding identity")
    model = identity["model"]
    if (getattr(args, "model", None) or model) != model or client.digest(model) != identity["digest"]:
        raise ValueError("embedding model changed; rebuild into a new collection")
    documents, sequence, space = collection_state(args, manifest)
    stored, unmanaged = {}, set()
    used = {int(d["id"]) for d in documents}
    for document in documents:
        if tag in document["tags"]:
            stored.setdefault(document["source"], []).append(document)
        else:
            unmanaged.add(document["source"])
    incoming = {source: [] for source in sources}
    for chunk in chunks:
        incoming[chunk.source].append(chunk)
    changed = unchanged = deleted = 0
    next_id = 0
    with tempfile.TemporaryDirectory(prefix="vecnook-sync-") as temporary:
        batch = Path(temporary) / "source.tsv"
        try:
            for source, source_chunks in incoming.items():
                if source in unmanaged:
                    raise ValueError(f"source belongs to an unmanaged import: {source}; use a new collection")
                old = sorted(stored.get(source, []), key=lambda d: int(d["id"]))
                if old:
                    next_id = max(next_id, int(old[-1]["id"]) + 1)
                old_content = [(d["text"], d["start_line"], d["end_line"], d["tags"]) for d in old]
                new_content = [(c.text, c.start_line, c.end_line, tags) for c in source_chunks]
                if old_content == new_content:
                    unchanged += 1
                    continue
                operation_count = max(len(old), len(source_chunks))
                if operation_count > 1024:
                    raise ValueError(f"source exceeds 1024-operation atomic batch: {source}")
                # Nothing for this source is written until all embeddings validate.
                vectors = []
                for offset in range(0, len(source_chunks), 16):
                    vectors.extend(client.embed(model, [document_input(model, c)
                                   for c in source_chunks[offset:offset + 16]]))
                if any(len(v) != manifest["dimensions"] for v in vectors):
                    raise ValueError("embedding dimensions changed")
                if client.digest(model) != identity["digest"]:
                    raise ValueError("embedding model changed during sync")
                rows, payload_bytes = [], 13
                for i, (chunk, vector) in enumerate(zip(source_chunks, vectors)):
                    if i < len(old):
                        identifier = int(old[i]["id"])
                    else:
                        while next_id in used:
                            next_id += 1
                        if next_id > 2**64 - 1:
                            raise ValueError("document ID space exhausted")
                        identifier = next_id
                        used.add(identifier)
                    payload = encode_document(identifier, chunk, tags)
                    payload_bytes += 13 + 4 * len(vector) + len(payload.encode())
                    rows.append(f"put\t{identifier}\t{coordinates(vector)}\t{payload}\n")
                for document in old[len(source_chunks):]:
                    rows.append(f"delete\t{document['id']}\n")
                    payload_bytes += 9
                body = "".join(rows)
                if payload_bytes > 8 * MAX_FILE or len(body.encode()) > 16 * MAX_FILE:
                    raise ValueError(f"source exceeds atomic batch byte limits: {source}")
                if rows:
                    batch.write_text(body, encoding="utf-8")
                    cli(args.binary, "docs-batch", *space, batch, "--if-sequence", sequence)
                    sequence += 1
                changed += 1
            # Pruning is restricted to this namespace and requires explicit opt-in.
            if getattr(args, "prune", False):
                for source in sorted(set(stored) - set(incoming)):
                    old = stored[source]
                    if len(old) > 1024:
                        raise ValueError(f"prune source exceeds 1024 operations: {source}")
                    batch.write_text("".join(f"delete\t{d['id']}\n" for d in old), encoding="utf-8")
                    cli(args.binary, "docs-batch", *space, batch, "--if-sequence", sequence)
                    sequence += 1
                    deleted += 1
        except (ValueError, OSError, subprocess.SubprocessError):
            print(f"Sync incomplete: changed={changed} unchanged={unchanged} deleted={deleted} failed=1; rerun sync", file=sys.stderr)
            raise
    # Source batches are already durable; checkpointing is optional maintenance.
    print(f"Sync complete: changed={changed} unchanged={unchanged} deleted={deleted} failed=0")


def search_documents(args, client):
    db = Path(args.db)
    manifest = json.loads(cli(args.binary, "docs-info", db.parent, db.name))
    identity = json.loads(manifest["model"])
    if identity.get("provider") != "ollama" or identity.get("prompt_version") != 1:
        raise ValueError("unsupported document embedding identity")
    model = identity["model"]
    if client.digest(model) != identity["digest"]:
        raise ValueError("embedding model changed; rebuild this document database")
    if not args.query.strip() or len(args.query.encode("utf-8")) > CHUNK_BYTES:
        raise ValueError("query must contain 1..3000 UTF-8 bytes")
    query = client.embed(model, [query_input(model, args.query)])[0]
    if len(query) != manifest["dimensions"]:
        raise ValueError("query embedding dimensions differ from the indexed model")
    filters = []
    for tag in getattr(args, "tags", []):
        filters.extend(["--tag", tag])
    if getattr(args, "source", None) is not None:
        filters.extend(["--source", args.source])
    response = json.loads(cli(args.binary, "docs-search", db.parent, db.name,
                              manifest["dimensions"], manifest["model"], "cosine",
                              coordinates(query), args.k, 128, "auto", *filters))
    result = response["search"]
    result["neighbors"] = [dict(match["document"], distance=match["distance"],
                                tags=[t for t in match["document"]["tags"] if not t.startswith(SYNC_PREFIX)])
                           for match in response["matches"]]
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
    index.add_argument("--tag", dest="tags", action="append", default=[])
    index.add_argument("--namespace", help="stable sync namespace when moving the document folder")
    sync = commands.add_parser("sync")
    sync.add_argument("documents")
    sync.add_argument("--db", required=True)
    sync.add_argument("--model", help="must match the existing model; inferred when omitted")
    sync.add_argument("--tag", dest="tags", action="append", default=[])
    sync.add_argument("--namespace", help="same namespace as the initial import")
    sync.add_argument("--prune", action="store_true", help="delete missing sources managed by this folder/namespace")
    search = commands.add_parser("search")
    search.add_argument("query")
    search.add_argument("--db", required=True)
    search.add_argument("--k", type=int, choices=range(1, 101), default=3, metavar="1..100")
    search.add_argument("--json", action="store_true")
    search.add_argument("--tag", dest="tags", action="append", default=[])
    search.add_argument("--source", help="exact source-relative path")
    args = parser.parse_args()
    try:
        client = Ollama(args.ollama_url)
        {"index": index_documents, "sync": sync_documents, "search": search_documents}[args.command](args, client)
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
