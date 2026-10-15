#!/usr/bin/env python3
"""Compare vector, BM25 and RRF on fixed EN/KO source judgments with local embeddings."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import struct
import sys
import time

from evaluate_demo import QUESTIONS

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "examples/documents"))
from search import Ollama, read_chunks, document_input, query_input, cli, coordinates, encode_document, retrieval_mode

# Fixed answers follow the described behavior; define them before retrieving results.
# The two Acme tools are fictional product-name fixtures, not external product claims.
EXTRA_DOCUMENTS = {
    "optimistic.md": """# Concurrent editing
write_batch_if_sequence accepts the sequence observed by a reader. If another write
committed first, Error::Conflict rejects the entire batch. Read the collection again
and reconcile the changed source before retrying. A successful call commits all
source chunks in one WAL frame. 버전 충돌을 피하려면 write_batch_if_sequence 호출에
읽었을 때의 sequence를 전달한다. 실패하면 최신 문서를 다시 읽는다.
""",
    "locking.md": """# One open database handle
Error::Locked means another handle or process owns the database directory. Keep
one Database or Collection alive and share it through application synchronization.
Close the competing handle before reopening. Do not remove the LOCK file to force
access. Error::Locked 오류는 이미 열린 핸들이 있다는 뜻이다. 기존 핸들을 닫는다.
""",
    "capacity.md": """# Physical node capacity
Error::Capacity includes the resource name, required count and limit. The physical_nodes
limit counts active and deleted versions. compact reclaims tombstones; capacity reports
remaining headroom. A completely full active collection must be split or reduced.
Error::Capacity physical_nodes 한도가 차면 compact로 삭제된 노드를 회수한다.
""",
    "model-change.md": """# Matching embedding spaces
Error::EmbeddingMismatch means the expected model identity, dimensions or metric
does not match the stored collection. Create a new named collection and re-embed
the original documents with the new model. Verify queries, then choose the new
collection in the application. Error::EmbeddingMismatch 오류는 모델 공간 불일치다.
""",
    "io-code.md": """# Application recovery code
This example application's ERR_IO_27 code means a durable write returned an I/O
error. Close the poisoned handle, reopen and inspect the affected document IDs.
The failed operation may have persisted. Reconcile those documents before retrying.
ERR_IO_27 재시도 절차는 핸들을 닫고 다시 열어서 저장된 문서를 확인하는 것이다.
""",
    "acme-sync.md": """# AcmeSync folder settings
AcmeSync is a fictional fixture tool for keeping local Markdown folders synchronized.
Choose one stable namespace per folder. Unchanged files skip embedding requests.
Enable prune only when missing files should be removed from that namespace.
AcmeSync 동기화 설정에는 문서 폴더와 namespace를 지정한다.
""",
    "acme-search.md": """# AcmeSearch query settings
AcmeSearch is a fictional fixture tool for searching an existing local document collection.
Use a source filter for one file, tags for a project, and hybrid ranking for exact
identifiers alongside meaning. Search results include original text and line ranges.
AcmeSearch 검색 설정에는 컬렉션 경로와 검색 질문을 지정한다.
""",
}
IDENTIFIER_QUESTIONS = [
    ("en", "write_batch_if_sequence", "optimistic.md"),
    ("ko", "write_batch_if_sequence로 변경 충돌을 피하려면?", "optimistic.md"),
    ("en", "Error::Locked", "locking.md"),
    ("ko", "Error::Locked 오류가 나면 어떤 핸들을 닫아야 해요?", "locking.md"),
    ("en", "Error::EmbeddingMismatch", "model-change.md"),
    ("ko", "Error::Capacity physical_nodes 한도가 차면?", "capacity.md"),
    ("en", "ERR_IO_27", "io-code.md"),
    ("ko", "ERR_IO_27 재시도 절차", "io-code.md"),
    ("en", "AcmeSync", "acme-sync.md"),
    ("ko", "AcmeSync 동기화 설정", "acme-sync.md"),
    ("en", "AcmeSearch", "acme-search.md"),
    ("ko", "AcmeSearch 검색 설정", "acme-search.md"),
]


def metrics(rows):
    return dict(queries=len(rows), hit_at_1=sum(r["rank"] == 1 for r in rows) / len(rows),
                recall_at_3=sum(r["rank"] is not None for r in rows) / len(rows),
                mrr_at_3=sum(1/r["rank"] if r["rank"] else 0 for r in rows) / len(rows))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default="target/release/vecnook")
    parser.add_argument("--model", default="embeddinggemma:latest")
    parser.add_argument("--output", required=True, help="new directory")
    args = parser.parse_args()
    root = Path(args.output).resolve()
    root.mkdir(parents=True, exist_ok=False)
    corpus = root / "corpus"
    shutil.copytree(ROOT / "examples/documents/sample", corpus)
    for source, text in EXTRA_DOCUMENTS.items():
        (corpus / source).write_text(text, encoding="utf-8")
    # A code catalog is deliberately repetitive: exact code identity must survive
    # otherwise equivalent meanings. Keep this deterministic, rather than mining
    # model failures for distractors. These are fictional application codes.
    for code in range(20, 40):
        if code != 27:
            (corpus / f"io-code-{code}.md").write_text(
                EXTRA_DOCUMENTS["io-code.md"].replace("ERR_IO_27", f"ERR_IO_{code}"), encoding="utf-8")
    questions = [(lang, query, source, "meaning") for lang, query, source in QUESTIONS]
    questions += [(lang, query, source, "identifier") for lang, query, source in IDENTIFIER_QUESTIONS]
    chunks = read_chunks(corpus)
    model = Ollama()
    digest = model.digest(args.model)
    vectors = model.embed(args.model, [document_input(args.model, c) for c in chunks]
                          + [query_input(args.model, q) for _, q, _, _ in questions])
    if model.digest(args.model) != digest:
        raise ValueError("model changed during evaluation")
    vectors = [[struct.unpack("<f", struct.pack("<f", value))[0] for value in row] for row in vectors]
    identity = json.dumps(dict(provider="ollama", model=args.model, digest=digest, prompt_version=1), separators=(",", ":"))
    space = [root, "documents", len(vectors[0]), identity, "cosine"]
    batch = root / "documents.tsv"
    batch.write_text("".join(f"put\t{i}\t{coordinates(v)}\t{encode_document(i, c, [])}\n"
                             for i, (c, v) in enumerate(zip(chunks, vectors))), encoding="utf-8")
    cli(args.binary, "docs-init", *space)
    cli(args.binary, "docs-batch", *space, batch)
    cli(args.binary, "docs-checkpoint", *space)
    rows = []
    for (language, query, expected, kind), vector in zip(questions, vectors[len(chunks):]):
        for mode in ["vector", "keyword", "hybrid", "auto"]:
            selected = retrieval_mode(query, mode)
            if selected == "vector":
                command = ["docs-search", *space, coordinates(vector), 3, 128, "exact"]
            elif selected == "keyword":
                command = ["docs-keywords", *space, query, 3]
            else:
                command = ["docs-hybrid", *space, coordinates(vector), query, 3]
            started = time.perf_counter()
            response = json.loads(cli(args.binary, *command))
            elapsed = (time.perf_counter() - started) * 1000
            sources = [match["document"]["source"] for match in response["matches"]]
            rank = sources.index(expected) + 1 if expected in sources else None
            rows.append(dict(language=language, kind=kind, query=query, expected=expected,
                             mode=mode, selected_mode=selected, returned=sources, rank=rank, cli_ms=elapsed))
    grouped = {}
    for mode in ["vector", "keyword", "hybrid", "auto"]:
        mode_rows = [r for r in rows if r["mode"] == mode]
        grouped[mode] = {"all": metrics(mode_rows)}
        for field, labels in [("language", ["en", "ko"]), ("kind", ["meaning", "identifier"])]:
            for label in labels:
                grouped[mode][label] = metrics([r for r in mode_rows if r[field] == label])
    report = dict(model=args.model, model_digest=digest, dimensions=len(vectors[0]), documents=len(chunks),
                  corpus={c.source: hashlib.sha256(c.text.encode()).hexdigest() for c in chunks},
                  vectors_sha256=hashlib.sha256(b"".join(struct.pack("<f", v) for row in vectors for v in row)).hexdigest(),
                  judgments_sha256=hashlib.sha256(json.dumps(questions, ensure_ascii=False).encode()).hexdigest(),
                  binary_sha256=hashlib.sha256(Path(args.binary).read_bytes()).hexdigest(),
                  rrf_k=60, candidates=64, metrics=grouped, questions=rows,
                  scope="22 fixed authored questions on 31 authored documents, including 20 deliberately repetitive fictional error-code entries; meaning questions retained from the existing evaluation. Source labels follow these documents. CLI timings include process startup, reopen and decoding; embedding excluded.")
    (root / "results.json").write_text(json.dumps(report, ensure_ascii=False, indent=2)+"\n", encoding="utf-8")
    print(json.dumps(grouped, ensure_ascii=False))


if __name__ == "__main__":
    main()
