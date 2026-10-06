#!/usr/bin/env python3
"""Build bounded fvecs from BEIR SciFact and a local Ollama model (stdlib only)."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import struct
import sys
from urllib.request import Request, urlopen
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "examples" / "documents"))
from search import Ollama, query_input  # noqa: E402

URL = "https://public.ukp.informatik.tu-darmstadt.de/thakur/BEIR/datasets/scifact.zip"
MD5 = "5f7d1de60b170fc8027bb7898e2efca1"
LIMIT = 32 * 1024 * 1024


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def prepare(args):
    output = Path(args.output)
    if output.exists():
        raise ValueError("choose a fresh output directory")
    request = Request(URL, headers={"User-Agent": "vecnook-benchmark/0.3 (https://github.com/gay00ung/vecnook)"})
    with urlopen(request, timeout=60) as response:
        archive = response.read(LIMIT + 1)
    if len(archive) > LIMIT or hashlib.md5(archive).hexdigest() != MD5:
        raise ValueError("SciFact download size/checksum differs from the published BEIR artifact")
    with zipfile.ZipFile(io.BytesIO(archive)) as files:
        def member(name):
            info = files.getinfo("scifact/" + name)
            if info.file_size > LIMIT:
                raise ValueError("dataset member too large")
            return files.read(info).decode("utf-8")
        corpus = [json.loads(line) for line in member("corpus.jsonl").splitlines()]
        queries = [json.loads(line) for line in member("queries.jsonl").splitlines()]
        test_ids = {line.split("\t")[0] for line in member("qrels/test.tsv").splitlines()[1:]}
    corpus = corpus[:args.count]
    queries = [row for row in queries if row["_id"] in test_ids][:args.queries]
    if not corpus or not queries:
        raise ValueError("dataset selection is empty")
    client = Ollama(args.ollama_url)
    model_digest = client.digest(args.model)
    output.mkdir(parents=True)
    dimensions = None
    truncated = 0

    def write_vectors(name, rows, document):
        nonlocal dimensions, truncated
        with (output / name).open("wb") as file:
            for offset in range(0, len(rows), 16):
                inputs = []
                for row in rows[offset:offset + 16]:
                    if document:
                        raw = row["text"].encode("utf-8")
                        truncated += int(len(raw) > 4000)
                        text = raw[:4000].decode("utf-8", errors="ignore")
                        title = row["title"].encode("utf-8")[:400].decode("utf-8", errors="ignore")
                        inputs.append(f"title: {title} | text: {text}" if args.model.split(":")[0] == "embeddinggemma" else text)
                    else:
                        inputs.append(query_input(args.model, row["text"]))
                vectors = client.embed(args.model, inputs)
                for vector in vectors:
                    if dimensions is None:
                        dimensions = len(vector)
                    if len(vector) != dimensions:
                        raise ValueError("embedding dimensions changed")
                    file.write(struct.pack("<I", dimensions))
                    file.write(struct.pack("<" + "f" * dimensions, *vector))
                print(f"{name}: {min(offset + 16, len(rows))}/{len(rows)}", flush=True)
    write_vectors("base.fvecs", corpus, True)
    write_vectors("query.fvecs", queries, False)
    if client.digest(args.model) != model_digest:
        raise ValueError("model changed during generation; discard this output")
    manifest = {"format": 1, "dataset": "BEIR SciFact", "source_url": URL,
                "source_sha256": hashlib.sha256(archive).hexdigest(), "source_md5": MD5,
                "model": args.model, "model_digest": model_digest, "dimensions": dimensions,
                "ollama_version": client.request("/api/version").get("version"),
                "count": len(corpus), "queries": len(queries), "query_split": "qrels/test.tsv",
                "corpus_ids": [row["_id"] for row in corpus], "query_ids": [row["_id"] for row in queries],
                "document_prompt": "title: TITLE | text: ABSTRACT" if args.model.split(":")[0] == "embeddinggemma" else "ABSTRACT",
                "query_prompt": "task: search result | query: CLAIM" if args.model.split(":")[0] == "embeddinggemma" else "CLAIM",
                "abstract_byte_limit": 4000, "title_byte_limit": 400, "truncated_abstracts": truncated,
                "base_sha256": digest(output / "base.fvecs"), "query_sha256": digest(output / "query.fvecs")}
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"Complete: {len(corpus)} documents, {len(queries)} independent test claims, {dimensions} dimensions", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True)
    parser.add_argument("--count", type=int, default=5183)
    parser.add_argument("--queries", type=int, default=300)
    parser.add_argument("--model", default="embeddinggemma:latest")
    parser.add_argument("--ollama-url", default="http://127.0.0.1:11434")
    args = parser.parse_args()
    if not 1 <= args.count <= 5183 or not 1 <= args.queries <= 300:
        parser.error("count must be 1..5183 and queries 1..300")
    try:
        prepare(args)
    except (ValueError, OSError, KeyError, zipfile.BadZipFile) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
