#!/usr/bin/env python3
"""Markdown search using Python's standard library, local Ollama and Vecnook."""

import argparse
import copy
from dataclasses import dataclass
from datetime import date, datetime, timezone
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
from urllib.error import URLError
import re

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
    file_bytes: int = 0
    modified: str = ""


def read_chunks(root, *, allow_empty=False, include_sources=False, chunker="bytes-v1", formats=(".md",)):
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
                elif path.suffix.lower() in formats:
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
        modified = datetime.fromtimestamp(path.stat().st_mtime, timezone.utc).date().isoformat()
        total += len(raw)
        if len(raw) > MAX_FILE or total > MAX_TOTAL:
            raise ValueError("document byte limit exceeded")
        source = path.relative_to(root).as_posix()
        sources.append(source)
        text, start, end, fenced = "", 1, 1, False
        for line_number, line in enumerate(raw.decode("utf-8").splitlines(keepends=True), 1):
            if chunker == "markdown-v1":
                heading = bool(re.match(r"^#{1,6}\s", line)) and not fenced
                boundary = heading or (not fenced and not line.strip() and len(text.encode("utf-8")) >= CHUNK_BYTES // 2)
                if boundary and text.strip():
                    chunks.append(Chunk(source, start, end, text, len(raw), modified))
                    text = ""
                if line.lstrip().startswith(("```", "~~~")):
                    fenced = not fenced
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
                        chunks.append(Chunk(source, start, end, text, len(raw), modified))
                    text = ""
                if not text:
                    start = line_number
                text += piece
                end = line_number
                if len(chunks) >= MAX_CHUNKS:
                    raise ValueError("document limit: 10,000 chunks")
        if text.strip():
            chunks.append(Chunk(source, start, end, text, len(raw), modified))
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
        try:
            with self.opener.open(request, timeout=120) as response:
                raw = response.read(16 * MAX_FILE + 1)
        except URLError as error:
            raise ValueError(f"Ollama request failed at {self.url}: {error}. "
                             "Start Ollama with 'ollama serve' and install the model with "
                             "'ollama pull embeddinggemma'.") from error
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
    if args and args[0] in {"docs-search", "docs-hybrid"} and len(args) > 6 and len(str(args[6])) > 8192:
        with tempfile.TemporaryDirectory(prefix="vecnook-query-") as temporary:
            vector = Path(temporary) / "query.csv"
            vector.write_text(str(args[6]), encoding="utf-8")
            arguments = list(args)
            arguments[6] = "@"+str(vector)
            return cli(binary, *arguments)
    result = subprocess.run([str(binary), *map(str, args)], capture_output=True, text=True,
                            encoding="utf-8", timeout=120)
    if result.returncode:
        raise ValueError(result.stderr.strip() or "Vecnook command failed")
    return result.stdout


def coordinates(vector):
    return ",".join(format(value, ".9g") for value in vector)


def encode_document(identifier, chunk, tags, numbers=None, dates=None):
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
    numbers, dates = numbers or {}, dates or {}
    if len(numbers) > 32 or len(dates) > 32:
        raise ValueError("at most 32 attributes per type")
    def field(key):
        if not key or "\0" in key:
            raise ValueError("attribute keys must be nonempty without NUL")
        return string(key, 128)
    if numbers or dates:
        data += struct.pack("<I", len(numbers))
        for key, value in sorted(numbers.items()):
            value = float(value)
            if not math.isfinite(value):
                raise ValueError("numeric attributes must be finite")
            data += field(key) + struct.pack("<d", 0.0 if value == 0 else value)
        data += struct.pack("<I", len(dates))
        for key, value in sorted(dates.items()):
            parsed = date.fromisoformat(value)
            if parsed.isoformat() != value:
                raise ValueError("date must be YYYY-MM-DD")
            data += field(key) + struct.pack("<I", parsed.year * 10000 + parsed.month * 100 + parsed.day)
    payload = ("VDOC2:" if numbers or dates else "VDOC1:") + data.hex()
    if len(payload) > 16 * 1024:
        raise ValueError("encoded document payload exceeds 16 KiB")
    return payload


def chunk_metadata(chunk, enabled):
    return ({"file_bytes": chunk.file_bytes}, {"modified": chunk.modified}) if enabled else ({}, {})


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
    chunker = getattr(args, "chunker", None) or "bytes-v1"
    chunks = read_chunks(args.documents, chunker=chunker, formats=input_formats(args))
    tags = import_tags(args)
    digest = client.digest(args.model)
    # Embed everything before creating storage, so model errors leave no partial database.
    vectors = []
    for offset in range(0, len(chunks), 16):
        vectors.extend(client.embed(args.model, [document_input(args.model, c)
                                                for c in chunks[offset:offset + 16]]))
        if getattr(args, "progress", True):
            print(f"Embedded {min(offset+16, len(chunks))}/{len(chunks)} chunks", file=sys.stderr)
    if any(len(v) != len(vectors[0]) for v in vectors):
        raise ValueError("model returned inconsistent embedding dimensions")
    if client.digest(args.model) != digest:
        raise ValueError("model changed while indexing; retry with a fixed model")
    settings = {"provider": "ollama", "model": args.model, "digest": digest, "prompt_version": 1}
    if chunker != "bytes-v1":
        settings.update(prompt_version=2, chunker=chunker)
    identity = json.dumps(settings, separators=(",", ":"))
    space = [db.parent, db.name, len(vectors[0]), identity, "cosine"]
    # Prepare and bound complete source batches before creating the database.
    with tempfile.TemporaryDirectory(prefix="vecnook-import-") as temporary:
        grouped = {}
        for index, (chunk, vector) in enumerate(zip(chunks, vectors)):
            payload = encode_document(index, chunk, tags, *chunk_metadata(chunk, getattr(args, "record_metadata", False)))
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
        for sequence, (source, batch) in enumerate(zip(grouped, prepared)):
            cli(args.binary, "docs-batch", *space, batch, "--if-sequence", sequence)
            if getattr(args, "progress", True):
                print(f"Synced {sequence+1}/{len(prepared)}: {source}", file=sys.stderr)
    cli(args.binary, "docs-checkpoint", *space)
    if not getattr(args, "sync_json", False):
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


def input_formats(args):
    return (".md", ".txt") if getattr(args, "include_text", False) else (".md",)


def inspected_state(args):
    """Read the authoritative view without trimming WAL or creating caches."""
    db = Path(args.db)
    return json.loads(cli(args.binary, "docs-inspect", db.parent, db.name))


def sync_documents(args, client):
    dry_run = getattr(args, "dry_run", False)
    db = Path(args.db)
    manifest = json.loads(cli(args.binary, "docs-info", db.parent, db.name)) if db.exists() else None
    identity = json.loads(manifest["model"]) if manifest else {}
    if manifest and (identity.get("provider") != "ollama" or identity.get("prompt_version") not in {1, 2}):
        raise ValueError("unsupported document embedding identity")
    model = (getattr(args, "model", None) or identity.get("model") or DEFAULT_MODEL)
    chunker = getattr(args, "chunker", None) or identity.get("chunker", "bytes-v1")
    if manifest and (model != identity["model"] or chunker != identity.get("chunker", "bytes-v1")):
        raise ValueError("embedding model/chunker changed; rebuild into a new collection with docs-reindex")
    chunks, sources = read_chunks(args.documents, allow_empty=True, include_sources=True,
                                 chunker=chunker, formats=input_formats(args))
    tag, tags = managed_tag(args), import_tags(args)
    state = inspected_state(args) if manifest else dict(documents=[], sequence="0", diagnostics={})
    documents, sequence = state["documents"], int(state["sequence"])
    if len(documents) > MAX_CHUNKS:
        raise ValueError("sync supports at most 10,000 stored chunks")
    stored, unmanaged = {}, set()
    used = {int(d["id"]) for d in documents}
    for document in documents:
        if tag in document["tags"]:
            stored.setdefault(document["source"], []).append(document)
        else:
            unmanaged.add(document["source"])
    record_metadata = getattr(args, "record_metadata", None)
    if record_metadata is None:
        record_metadata = any("file_bytes" in d.get("numbers", {}) or "modified" in d.get("dates", {})
                              for docs in stored.values() for d in docs)
    incoming = {source: [] for source in sources}
    for chunk in chunks:
        incoming[chunk.source].append(chunk)
    plans, unchanged, next_id = [], 0, 0
    for source, source_chunks in incoming.items():
        if source in unmanaged:
            raise ValueError(f"source belongs to an unmanaged import: {source}; use a new collection")
        old = sorted(stored.get(source, []), key=lambda d: int(d["id"]))
        if old:
            next_id = max(next_id, int(old[-1]["id"]) + 1)
        old_content = [(d["text"], d["start_line"], d["end_line"], d["tags"], d.get("numbers", {}), d.get("dates", {})) for d in old]
        new_content = [(c.text, c.start_line, c.end_line, tags, *chunk_metadata(c, record_metadata)) for c in source_chunks]
        if old_content == new_content:
            unchanged += 1
            continue
        if max(len(old), len(source_chunks)) > 1024:
            raise ValueError(f"source exceeds 1024-operation atomic batch: {source}")
        reusable = {}
        for d in old:
            old_chunk = Chunk(d["source"], d["start_line"], d["end_line"], d["text"])
            reusable[document_input(model, old_chunk)] = d["vector"]
        vectors = [reusable.get(document_input(model, c)) for c in source_chunks]
        rows = []
        for i, c in enumerate(source_chunks):
            if i < len(old):
                identifier = int(old[i]["id"])
            else:
                while next_id in used:
                    next_id += 1
                if next_id > 2**64-1:
                    raise ValueError("document ID space exhausted")
                identifier = next_id
                used.add(identifier)
                next_id += 1
            rows.append((identifier, c, encode_document(identifier, c, tags, *chunk_metadata(c, record_metadata))))
        plans.append(dict(source=source, old=old, rows=rows, vectors=vectors))
    removed = sorted(set(stored) - set(incoming)) if getattr(args, "prune", False) else []
    for source in removed:
        if len(stored[source]) > 1024:
            raise ValueError(f"prune source exceeds 1024 operations: {source}")
    dimensions = manifest["dimensions"] if manifest else None
    puts = sum(len(p["rows"]) for p in plans)
    # Conservative upper bound: existing physical rows plus every replacement/insert.
    estimated = None if dimensions is None else state["diagnostics"]["estimated_snapshot_bytes"] + sum(
        13 + 4 * dimensions + len(payload.encode()) for p in plans for _, _, payload in p["rows"])
    report = dict(schema_version=1, dry_run=dry_run, namespace=tag, changed=len(plans), unchanged=unchanged,
                  deleted=len(removed), failed=0, files=[dict(source=p["source"], chunks=len(p["rows"]),
                  reembed=sum(v is None for v in p["vectors"])) for p in plans], deleted_sources=removed,
                  reembed=sum(v is None for p in plans for v in p["vectors"]),
                  reused_vectors=sum(v is not None for p in plans for v in p["vectors"]),
                  dimensions=dimensions, estimated_snapshot_bytes_upper_bound=estimated,
                  estimated_physical_nodes=state["diagnostics"].get("physical_nodes", 0)+puts,
                  snapshot_limit_bytes=256*MAX_FILE, physical_node_limit=100000,
                  current_diagnostics=state["diagnostics"])
    if dry_run:
        if getattr(args, "sync_json", False):
            print(json.dumps(report, ensure_ascii=False, indent=2))
        else:
            print(f"Sync plan (no writes): changed={len(plans)} unchanged={unchanged} deleted={len(removed)} reembed={report['reembed']} reused={report['reused_vectors']}")
            for p in report["files"]:
                print(f"  {p['source']}: chunks={p['chunks']} reembed={p['reembed']}")
            for source in removed:
                print(f"  delete {source}")
            print("Capacity: "+(f"snapshot upper bound={estimated} bytes" if estimated is not None else "dimensions and snapshot estimate unknown until the first embedding"))
        return report
    if not manifest:
        if not chunks:
            raise ValueError("no nonempty Markdown documents found")
        args.model = model
        changed = index_documents(args, client)
        report.update(changed=changed)
        if getattr(args, "sync_json", False):
            print(json.dumps(report, ensure_ascii=False))
        else:
            print(f"Sync complete: changed={changed} unchanged=0 deleted=0 failed=0")
        return report
    if client.digest(model) != identity["digest"]:
        raise ValueError("embedding model changed; rebuild into a new collection")
    space = [db.parent, db.name, dimensions, manifest["model"], "cosine"]
    changed = deleted = 0
    with tempfile.TemporaryDirectory(prefix="vecnook-sync-") as temporary:
        batch = Path(temporary) / "source.tsv"
        try:
            for p in plans:
                missing = [i for i, v in enumerate(p["vectors"]) if v is None]
                for offset in range(0, len(missing), 16):
                    indices = missing[offset:offset+16]
                    embedded = client.embed(model, [document_input(model, p["rows"][i][1]) for i in indices])
                    for i, vector in zip(indices, embedded):
                        p["vectors"][i] = vector
                if any(v is None or len(v) != dimensions for v in p["vectors"]):
                    raise ValueError("embedding dimensions changed")
                if client.digest(model) != identity["digest"]:
                    raise ValueError("embedding model changed during sync")
                rows, payload_bytes = [], 13
                for (identifier, chunk, payload), vector in zip(p["rows"], p["vectors"]):
                    rows.append(f"put\t{identifier}\t{coordinates(vector)}\t{payload}\n")
                    payload_bytes += 13 + 4*len(vector) + len(payload.encode())
                for d in p["old"][len(p["rows"]):]:
                    rows.append(f"delete\t{d['id']}\n")
                    payload_bytes += 9
                body = "".join(rows)
                if payload_bytes > 8*MAX_FILE or len(body.encode()) > 16*MAX_FILE:
                    raise ValueError(f"source exceeds atomic batch byte limits: {p['source']}")
                if rows:
                    batch.write_text(body, encoding="utf-8")
                    cli(args.binary, "docs-batch", *space, batch, "--if-sequence", sequence)
                    sequence += 1
                changed += 1
                if getattr(args, "progress", True):
                    print(f"Synced {changed}/{len(plans)}: {p['source']}", file=sys.stderr)
            for source in removed:
                batch.write_text("".join(f"delete\t{d['id']}\n" for d in stored[source]), encoding="utf-8")
                cli(args.binary, "docs-batch", *space, batch, "--if-sequence", sequence)
                sequence += 1
                deleted += 1
        except (ValueError, OSError, subprocess.SubprocessError):
            print(f"Sync incomplete: changed={changed} unchanged={unchanged} deleted={deleted} failed=1; rerun sync", file=sys.stderr)
            raise
    if getattr(args, "sync_json", False):
        print(json.dumps(report, ensure_ascii=False, indent=2))
    else:
        print(f"Sync complete: changed={changed} unchanged={unchanged} deleted={deleted} failed=0 reembed={report['reembed']} reused={report['reused_vectors']}")
    return report


def retrieval_mode(query, requested):
    """Use RRF for explicit identifiers; preserve vector ranking for prose questions."""
    if requested != "auto":
        return requested
    tokens = re.findall(r"[A-Za-z0-9_:-]+", query)
    identifier = any("_" in t or "::" in t
                     or (any(c.isdigit() for c in t) and any(c.isalpha() for c in t))
                     or re.search(r"[A-Z][a-z]+[A-Z]", t) for t in tokens)
    single = bool(re.fullmatch(r"[A-Za-z][A-Za-z0-9_:-]*", query.strip()))
    return "hybrid" if identifier or single else "vector"


def search_documents(args, client, reranker=None):
    db = Path(args.db)
    manifest = json.loads(cli(args.binary, "docs-info", db.parent, db.name))
    if not args.query.strip() or len(args.query.encode("utf-8")) > CHUNK_BYTES:
        raise ValueError("query must contain 1..3000 UTF-8 bytes")
    requested_mode = getattr(args, "mode", "vector")
    mode = retrieval_mode(args.query, requested_mode)
    maximum = getattr(args, "max_distance", None)
    if maximum is not None and (mode == "keyword" or not math.isfinite(maximum)):
        raise ValueError("max distance requires vector/hybrid mode and a finite bound")
    identity, model = {}, None
    if mode != "keyword":
        identity = json.loads(manifest["model"])
        if identity.get("provider") != "ollama" or identity.get("prompt_version") not in {1, 2}:
            raise ValueError("text embeddings require a supported Ollama identity; use keyword mode or caller-provided vectors")
        model = identity["model"]
    if mode != "keyword":
        if client.digest(model) != identity["digest"]:
            raise ValueError("embedding model changed; rebuild this document database")
        query = client.embed(model, [query_input(model, args.query)])[0]
        if len(query) != manifest["dimensions"]:
            raise ValueError("query embedding dimensions differ from the indexed model")
    filters = []
    if mode != "vector" and getattr(args, "analyzer", "words") != "words":
        filters += ["--analyzer", args.analyzer]
    requested_k = args.k
    candidate_k = 100 if getattr(args, "group_by_source", False) or getattr(args, "max_distance", None) is not None or reranker else requested_k
    for tag in getattr(args, "tags", []):
        filters.extend(["--tag", tag])
    if getattr(args, "source", None) is not None:
        filters.extend(["--source", args.source])
    if getattr(args, "where", None) is not None:
        filters.extend(["--where", args.where])
    space = [db.parent, db.name, manifest["dimensions"], manifest["model"], {"cosine":"cosine", "squared_l2":"l2", "negative_inner_product":"ip"}[manifest["metric"]]]
    if mode == "keyword":
        response = json.loads(cli(args.binary, "docs-keywords", *space, args.query, candidate_k, *filters))
        result = dict(schema_version=1, mode=mode, **response["keyword"])
    elif mode == "hybrid":
        response = json.loads(cli(args.binary, "docs-hybrid", *space, coordinates(query),
                              args.query, candidate_k, "--candidates", max(64, candidate_k), *filters))
        result = dict(schema_version=1, mode=mode, complete=response["complete"],
                      candidate_count=response["candidate_count"], rrf_k=response["rrf_k"],
                      vector=response["search"], keyword=response["keyword"])
    else:
        response = json.loads(cli(args.binary, "docs-search", *space, coordinates(query),
                                  candidate_k, 128, "auto", *filters))
        result = response["search"]
    result["neighbors"] = [dict(match["document"],
                                **{key: value for key, value in match.items() if key != "document"},
                                tags=[t for t in match["document"]["tags"] if not t.startswith(SYNC_PREFIX)])
                           for match in response["matches"]]
    if requested_mode == "auto":
        result["retrieval_mode"] = mode
    result["retrieval_reason"] = ("explicit_mode" if requested_mode != "auto" else
                                   "identifier_heuristic" if mode == "hybrid" else "prose_heuristic")
    result["analyzer"] = getattr(args, "analyzer", "words")
    if candidate_k != requested_k:
        result["candidate_complete"] = result.get("complete")
    if reranker:
        before = {n["id"] for n in result["neighbors"]}
        reordered = list(reranker(args.query, copy.deepcopy(result["neighbors"])))
        if len(reordered) != len(before) or {n["id"] for n in reordered} != before:
            raise ValueError("reranker must return every candidate exactly once")
        # Accept only order changes; preserve original payloads, filters and scores.
        original = {n["id"]: n for n in result["neighbors"]}
        result["neighbors"] = [original[n["id"]] for n in reordered]
        result["reranker_applied"] = True
    maximum = getattr(args, "max_distance", None)
    if maximum is not None:
        if not math.isfinite(maximum):
            raise ValueError("max distance must be finite")
        result["neighbors"] = [n for n in result["neighbors"] if n.get("distance") is not None and n["distance"] <= maximum]
    if getattr(args, "group_by_source", False):
        seen, selected = set(), []
        for n in result["neighbors"]:
            if n["source"] not in seen:
                selected.append(n)
                seen.add(n["source"])
        result["neighbors"] = selected
    result["neighbors"] = result["neighbors"][:requested_k]
    if candidate_k != requested_k and "complete" in result:
        eligible = result.get("eligible_count", result.get("vector", {}).get("eligible_count", 0))
        result["complete"] = len(result["neighbors"]) == min(requested_k, eligible)
    context = getattr(args, "context_chunks", 0)
    if context:
        context_flags = []
        for tag in getattr(args, "tags", []):
            context_flags += ["--tag", tag]
        if getattr(args, "source", None) is not None:
            context_flags += ["--source", args.source]
        if getattr(args, "where", None) is not None:
            context_flags += ["--where", args.where]
        for n in result["neighbors"]:
            n["context"] = json.loads(cli(args.binary, "docs-context", *space, n["id"], context, *context_flags))["documents"]
    if args.json:
        print(json.dumps(result, ensure_ascii=False, indent=2))
    else:
        print(f"Search mode={mode} reason={result['retrieval_reason']} analyzer={result['analyzer']}")
        if not result["neighbors"]:
            eligible = result.get("eligible_count", result.get("vector", {}).get("eligible_count", 0))
            print("No results: filters select no documents." if eligible == 0 else
                  "No results: no matching keyword tokens or candidates within the selected distance bound.")
        if getattr(args, "explain", False):
            details = {k: v for k, v in result.items() if k != "neighbors"}
            print("Diagnostics (scores are not confidence percentages): "+json.dumps(details, ensure_ascii=False))
        for neighbor in result["neighbors"]:
            score = (f"score={neighbor['score']:.6f}" if "score" in neighbor
                     else f"distance={neighbor['distance']:.6f}")
            print(f"{neighbor['source']}:{neighbor['start_line']}-{neighbor['end_line']} {score}")
            print(neighbor["text"].strip() + "\n")
            for context in neighbor.get("context", []):
                if context["id"] != neighbor["id"]:
                    print(f"Context {context['start_line']}-{context['end_line']}: {context['text'].strip()}")


def environment_check(args, client):
    checks = [dict(kind="python", ready=sys.version_info >= (3, 10), version=sys.version.split()[0],
                   hint="Install Python 3.10+ and set VECNOOK_PYTHON if needed")]
    model = getattr(args, "model", None) or DEFAULT_MODEL
    expected = None
    if getattr(args, "db", None):
        db = Path(args.db)
        info = json.loads(cli(args.binary, "docs-info", db.parent, db.name))
        identity = json.loads(info["model"])
        model, expected = identity["model"], identity["digest"]
    try:
        digest = client.digest(model)
        ready = expected is None or digest == expected
        checks.append(dict(kind="model" if ready else "model_mismatch", ready=ready, model=model,
                           hint="Ready" if ready else "Use docs-reindex into a new collection; keep the old collection for rollback"))
    except ValueError as error:
        checks.append(dict(kind="model_missing" if "not installed" in str(error) else "model_server",
                           ready=False, model=model, hint=str(error)))
    result = dict(schema_version=1, ready=all(c["ready"] for c in checks), checks=checks)
    if getattr(args, "json", False):
        print(json.dumps(result, ensure_ascii=False, indent=2))
    else:
        for check in checks:
            print(f"{check['kind']}: {'ready' if check['ready'] else 'action required'}; {check['hint']}")
    return 0 if result["ready"] else 1


def export_jsonl(args, _client):
    db = Path(args.db)
    info = json.loads(cli(args.binary, "docs-info", db.parent, db.name))
    state = inspected_state(args)
    # x mode refuses overwriting an existing archive.
    with Path(args.output).open("x", encoding="utf-8", newline="\n") as output:
        output.write(json.dumps(dict(type="vecnook_collection", schema_version=1, space=info), ensure_ascii=False)+"\n")
        for d in state["documents"]:
            output.write(json.dumps(d, ensure_ascii=False, allow_nan=False)+"\n")
    print(f"Exported {len(state['documents'])} documents")


def import_jsonl(args, _client):
    db = Path(args.db)
    if db.exists():
        raise ValueError("JSONL import requires a new collection directory")
    source = Path(args.input)
    if source.stat().st_size > 512*MAX_FILE:
        raise ValueError("JSONL input exceeds 512 MiB")
    with source.open(encoding="utf-8") as stream:
        header = json.loads(stream.readline(64*1024))
        if not isinstance(header, dict) or header.get("type") != "vecnook_collection" or type(header.get("schema_version")) is not int or header["schema_version"] != 1:
            raise ValueError("unsupported JSONL collection header")
        info, rows, seen, encoded = header["space"], [], set(), 56
        if not isinstance(info, dict) or not isinstance(info.get("model"), str):
            raise ValueError("invalid JSONL embedding identity")
        dimension = info["dimensions"]
        if type(dimension) is not int or not 1 <= dimension <= 4096:
            raise ValueError("invalid JSONL dimensions")
        metric = {"cosine": "cosine", "squared_l2": "l2", "negative_inner_product": "ip"}.get(info["metric"])
        if metric is None:
            raise ValueError("unsupported JSONL metric")
        while True:
            line = stream.readline(128*1024)
            if not line:
                break
            if not line.endswith("\n") or len(line.encode()) > 128*1024:
                raise ValueError("JSONL row too long or missing final newline")
            d = json.loads(line)
            if not isinstance(d, dict) or any(not isinstance(d.get(k), str) for k in ["id", "source", "text"]) or not isinstance(d.get("vector"), list) or not isinstance(d.get("tags"), list) or any(not isinstance(tag, str) for tag in d["tags"]):
                raise ValueError("invalid JSONL document fields")
            if not isinstance(d["id"], str) or not re.fullmatch(r"0|[1-9][0-9]*", d["id"]):
                raise ValueError("JSONL IDs must be canonical decimal strings")
            identifier, vector = int(d["id"]), d["vector"]
            if identifier in seen or len(seen) >= MAX_CHUNKS:
                raise ValueError("duplicate ID or too many JSONL chunks")
            seen.add(identifier)
            if len(vector) != dimension or any(type(v) not in {int, float} or not math.isfinite(v) or abs(v) > 3.4028234663852886e38 for v in vector) or (metric == "cosine" and not any(vector)):
                raise ValueError("invalid JSONL vector")
            if type(d["start_line"]) is not int or type(d["end_line"]) is not int or not 1 <= d["start_line"] <= d["end_line"] <= 2**32-1:
                raise ValueError("invalid JSONL line range")
            c = Chunk(d["source"], d["start_line"], d["end_line"], d["text"])
            if not c.source or "\0" in c.source:
                raise ValueError("invalid JSONL source")
            numbers, dates = d.get("numbers", {}), d.get("dates", {})
            if not isinstance(numbers, dict) or not isinstance(dates, dict) or any(not isinstance(k, str) or type(v) not in {int, float} or not math.isfinite(v) for k, v in numbers.items()) or any(not isinstance(k, str) or not isinstance(v, str) for k, v in dates.items()):
                raise ValueError("invalid JSONL typed attributes")
            payload = encode_document(identifier, c, d["tags"], numbers, dates)
            encoded += 13+4*dimension+len(payload.encode())
            if encoded > 256*MAX_FILE:
                raise ValueError("JSONL snapshot capacity exceeded")
            rows.append(f"put\t{identifier}\t{coordinates(vector)}\t{payload}\n")
    # All rows validate before the destination is claimed. Use a staged native
    # collection and a native logical archive for fully validated single-snapshot import.
    with tempfile.TemporaryDirectory(prefix="vecnook-jsonl-") as temporary:
        root = Path(temporary)
        space = [root, "staged", dimension, info["model"], metric]
        cli(args.binary, "docs-init", *space)
        for offset in range(0, len(rows), 128):
            batch = root / "batch.tsv"
            batch.write_text("".join(rows[offset:offset+128]), encoding="utf-8")
            cli(args.binary, "docs-batch", *space, batch)
        archive = root / "transfer.bin"
        cli(args.binary, "docs-export", *space, archive)
        cli(args.binary, "docs-import", archive, db.parent, db.name)
    print(f"Imported {len(rows)} documents")


def reindex_documents(args, client):
    old, destination = Path(args.db), Path(args.to)
    if destination.resolve().is_relative_to(old.resolve()):
        raise ValueError("reindex destination must be outside the original collection, including with --resume")
    if destination.exists() and not getattr(args, "resume", False):
        raise ValueError("reindex needs a fresh --to path, or explicit --resume for a partial new collection")
    original = inspected_state(args)
    # Reindex regenerates from the selected folder, never mutates the old space.
    args.db = destination
    try:
        result = sync_documents(args, client)
        target = inspected_state(args)
        manifest = json.loads(cli(args.binary, "docs-info", destination.parent, destination.name))
        chunker = json.loads(manifest["model"]).get("chunker", "bytes-v1")
        expected = [(c.source, c.start_line, c.end_line, c.text) for c in read_chunks(args.documents, chunker=chunker, formats=input_formats(args))]
        actual = [(d["source"], d["start_line"], d["end_line"], d["text"]) for d in target["documents"]]
        if sorted(expected) != sorted(actual):
            raise ValueError("reindex verification failed: original chunks differ; old collection is preserved")
        if int(inspected_state(type("Old", (), {"db": old, "binary": args.binary})())["sequence"]) != int(original["sequence"]):
            raise ValueError("old collection changed concurrently; inspect before switching paths")
        print(f"Reindex verified: {len(target['documents'])} chunks; old collection preserved at {old}. Switch your app path to {destination}; rollback uses {old}.", file=sys.stderr)
        return result
    finally:
        args.db = old


def main():
    parser = argparse.ArgumentParser(prog="vecnook", description=__doc__)
    parser.add_argument("--binary", default="vecnook", help="path to the installed Vecnook CLI")
    parser.add_argument("--ollama-url", default="http://127.0.0.1:11434")
    commands = parser.add_subparsers(dest="command", required=True)
    env = commands.add_parser("env-check", help="check Python, local model server and model identity")
    env.add_argument("--model")
    env.add_argument("--db")
    env.add_argument("--json", action="store_true")
    export = commands.add_parser("export-jsonl")
    export.add_argument("--db", required=True)
    export.add_argument("--output", required=True)
    imported = commands.add_parser("import-jsonl")
    imported.add_argument("input")
    imported.add_argument("--db", required=True)
    reindex = commands.add_parser("reindex")
    reindex.add_argument("documents")
    reindex.add_argument("--db", required=True)
    reindex.add_argument("--to", required=True)
    reindex.add_argument("--model", required=True)
    reindex.add_argument("--resume", action="store_true")
    reindex.add_argument("--namespace")
    reindex.add_argument("--tag", dest="tags", action="append", default=[])
    reindex.add_argument("--json", dest="sync_json", action="store_true")
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
    search.add_argument("--explain", action="store_true")
    search.add_argument("--analyzer", choices=["words", "korean-ngram"], default="words")
    search.add_argument("--group-by-source", action="store_true")
    search.add_argument("--max-distance", type=float)
    search.add_argument("--context-chunks", type=int, choices=range(0, 4), default=0)
    search.add_argument("--db", required=True)
    search.add_argument("--k", type=int, choices=range(1, 101), default=3, metavar="1..100")
    search.add_argument("--json", action="store_true")
    search.add_argument("--tag", dest="tags", action="append", default=[])
    search.add_argument("--source", help="exact source-relative path")
    search.add_argument("--where", help='Boolean expression, e.g. tag("rust") AND number("file_bytes") >= 100')
    search.add_argument("--mode", choices=["auto", "vector", "keyword", "hybrid"], default="vector",
                        help="auto uses hybrid for identifiers and vector for prose questions")
    sync.add_argument("--dry-run", action="store_true")
    sync.add_argument("--json", dest="sync_json", action="store_true")
    sync.add_argument("--progress", action=argparse.BooleanOptionalAction, default=True)
    for command in [index, sync, reindex]:
        command.add_argument("--chunker", choices=["bytes-v1", "markdown-v1"])
        command.add_argument("--include-text", action="store_true", help="also read UTF-8 .txt files")
        command.add_argument("--record-metadata", action=argparse.BooleanOptionalAction, default=None,
                             help="record file_bytes and UTC modified date; sync infers the existing setting")
    # Native docs-sync/docs-find keep the origin next to the command's own flags.
    for command in [index, sync, search, env, export, imported, reindex]:
        command.add_argument("--ollama-url", default=argparse.SUPPRESS,
                             help="local Ollama HTTP origin")
    native = os.environ.get("VECNOOK_COMMAND")
    mapping = {"docs-sync":"sync", "docs-find":"search", "env-check":"env-check", "docs-export-jsonl":"export-jsonl", "docs-import-jsonl":"import-jsonl", "docs-reindex":"reindex"}
    if native in mapping:
        commands.choices[mapping[native]].prog = "vecnook "+native
    args = parser.parse_args()
    try:
        client = Ollama(args.ollama_url)
        handler = {"index": index_documents, "sync": sync_documents, "search": search_documents,
                   "env-check": environment_check, "export-jsonl": export_jsonl,
                   "import-jsonl": import_jsonl, "reindex": reindex_documents}[args.command]
        result = handler(args, client)
        if args.command == "env-check":
            return result
    except (ValueError, OSError, KeyError, TypeError, OverflowError, struct.error, subprocess.SubprocessError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
