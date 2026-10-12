#!/usr/bin/env python3
"""Evaluate authored EN/KO questions with genuine local embeddings and exact search."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "examples/documents"))
from search import Ollama, read_chunks, document_input, query_input, cli, coordinates, encode_document

# One expected source per question, authored before measuring the model.
QUESTIONS = [
    ("en", "Which model information must match when I search my documents?", "embeddings.md"),
    ("en", "How can I restrict a query to one project's tags?", "filtering.md"),
    ("en", "How do I reclaim deleted vector nodes and make an independent backup?", "maintenance.md"),
    ("en", "What happens to acknowledged writes if the process crashes?", "recovery.md"),
    ("en", "How does increasing efSearch affect recall and latency?", "tuning.md"),
    ("ko", "문서와 검색 질문의 임베딩 모델이 달라지면 어떻게 해야 하나요?", "embeddings.md"),
    ("ko", "특정 프로젝트 태그가 있는 문서 안에서만 검색하려면?", "filtering.md"),
    ("ko", "삭제된 벡터의 공간을 회수하고 독립적인 백업을 만드는 방법은?", "maintenance.md"),
    ("ko", "프로세스가 갑자기 종료되면 이미 성공한 쓰기는 복구되나요?", "recovery.md"),
    ("ko", "efSearch를 높이면 검색 정확도와 속도에 어떤 영향이 있나요?", "tuning.md"),
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default="target/release/vecnook")
    parser.add_argument("--model", default="embeddinggemma:latest")
    parser.add_argument("--output", required=True, help="new directory; never overwrite a collection")
    args = parser.parse_args()
    root = Path(args.output).resolve()
    if root.exists():
        parser.error("output must be a new directory")
    model = Ollama()
    digest = model.digest(args.model)
    chunks = read_chunks(ROOT / "examples/documents/sample")
    vectors = model.embed(args.model, [document_input(args.model, c) for c in chunks]
                          + [query_input(args.model, q) for _, q, _ in QUESTIONS])
    if model.digest(args.model) != digest:
        raise ValueError("model changed during evaluation")
    vectors = [[struct.unpack("<f", struct.pack("<f", v))[0] for v in row] for row in vectors]
    root.mkdir(parents=True)
    identity = json.dumps(dict(provider="ollama", model=args.model, digest=digest, prompt_version=1), separators=(",", ":"))
    space = [root, "documents", len(vectors[0]), identity, "cosine"]
    batch = root / "documents.tsv"
    batch.write_text("".join(f"put\t{i}\t{coordinates(v)}\t{encode_document(i, c, [])}\n"
                             for i, (c, v) in enumerate(zip(chunks, vectors))), encoding="utf-8")
    cli(args.binary, "docs-init", *space)
    cli(args.binary, "docs-batch", *space, batch)
    rows = []
    for (language, query, expected), vector in zip(QUESTIONS, vectors[len(chunks):]):
        result = json.loads(cli(args.binary, "docs-search", *space, coordinates(vector), 3, 128, "exact"))
        sources = [m["document"]["source"] for m in result["matches"]]
        rank = sources.index(expected) + 1 if expected in sources else None
        rows.append(dict(language=language, query=query, expected=expected, returned=sources,
                         hit_at_1=rank == 1, reciprocal_rank_at_3=1/rank if rank else 0))
    metrics = {lang: dict(queries=len(group), hit_at_1=sum(r["hit_at_1"] for r in group)/len(group),
                         mrr_at_3=sum(r["reciprocal_rank_at_3"] for r in group)/len(group))
               for lang in ["en", "ko"] if (group := [r for r in rows if r["language"] == lang])}
    report = dict(model=args.model, model_digest=digest, dimensions=len(vectors[0]), documents=len(chunks),
        vectors_sha256=hashlib.sha256(b"".join(struct.pack("<f", v) for row in vectors for v in row)).hexdigest(),
        sources={c.source: hashlib.sha256(c.text.encode()).hexdigest() for c in chunks},
        strategy="exact", metrics=metrics, questions=rows,
        scope="Ten authored questions on five authored English documents; not an independent multilingual benchmark")
    (root / "results.json").write_text(json.dumps(report, ensure_ascii=False, indent=2)+"\n", encoding="utf-8")
    print(json.dumps(metrics, ensure_ascii=False))


if __name__ == "__main__":
    main()
