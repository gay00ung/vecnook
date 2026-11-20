#!/usr/bin/env python3
"""Extract static HTML text into JSONL v1 with an application-supplied vector."""
import argparse
from html.parser import HTMLParser
from itertools import islice
import json
from pathlib import Path
import re

MAX_INPUT_BYTES = 1024 * 1024
MAX_VECTOR_BYTES = 128 * 1024
MAX_TEXT_BYTES = 6000
F32_OVERFLOW = float.fromhex("0x1.ffffffp+127")
METRICS = {"l2": "squared_l2", "ip": "negative_inner_product", "cosine": "cosine"}
BLOCKS = {
    "address", "article", "aside", "blockquote", "br", "dd", "div", "dl", "dt",
    "fieldset", "figcaption", "figure", "footer", "form", "h1", "h2", "h3", "h4",
    "h5", "h6", "header", "hr", "li", "main", "nav", "ol", "p", "pre", "section",
    "table", "td", "th", "tr", "ul",
}
SKIP = {"head", "script", "style", "template", "noscript"}


class _StaticText(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.parts = []
        self.skipped = []

    def boundary(self):
        if self.parts and not self.parts[-1].endswith("\n"):
            self.parts.append("\n")

    def handle_starttag(self, tag, attrs):
        if self.skipped:
            if tag in SKIP:
                self.skipped.append(tag)
        elif tag in SKIP:
            self.skipped.append(tag)
        elif tag in BLOCKS:
            self.boundary()

    def handle_endtag(self, tag):
        if self.skipped:
            if self.skipped[-1] == tag:
                self.skipped.pop()
        elif tag in BLOCKS:
            self.boundary()

    def handle_data(self, data):
        if data and not self.skipped:
            self.parts.append(data)


def extract_text(html):
    """Apply the static text-node/whitespace contract in the example README."""
    if not isinstance(html, str) or len(html.encode("utf-8")) > MAX_INPUT_BYTES:
        raise ValueError("HTML must be UTF-8 text of at most 1 MiB")
    parser = _StaticText()
    parser.feed(html)
    parser.close()
    text = "".join(parser.parts).strip("\n")
    if not text.strip():
        raise ValueError("HTML contains no extracted text")
    if len(text.encode("utf-8")) > MAX_TEXT_BYTES:
        raise ValueError("Extracted HTML text exceeds 6000 UTF-8 bytes; chunk it in the application")
    return text


def _field(value, label, limit):
    if not isinstance(value, str) or not value or "\0" in value:
        raise ValueError(f"{label} must be a nonempty string without NUL")
    if len(value.encode("utf-8")) > limit:
        raise ValueError(f"{label} exceeds {limit} UTF-8 bytes")


def collection_jsonl(html, *, identifier, source, vector, model_identity, metric="l2", tags=()):
    """Return one collection header and one document; never choose an embedding model."""
    if (not isinstance(identifier, str) or len(identifier) > 20
            or re.fullmatch(r"0|[1-9][0-9]*", identifier) is None
            or int(identifier) > 2**64 - 1):
        raise ValueError("identifier must be a canonical u64 decimal string")
    _field(source, "source", 1024)
    _field(model_identity, "model_identity", 512)
    if (not isinstance(vector, list) or not 1 <= len(vector) <= 4096
            or any(type(x) not in (int, float) or not -F32_OVERFLOW < x < F32_OVERFLOW
                   for x in vector)):
        raise ValueError("vector must contain 1..4096 finite f32-roundable coordinates")
    # Half the smallest f32 subnormal rounds to zero (ties to even).
    if (not isinstance(metric, str) or metric not in METRICS
            or (metric == "cosine" and not any(abs(x) > 2**-150 for x in vector))):
        raise ValueError("metric or cosine vector is invalid")
    if isinstance(tags, str):
        raise ValueError("tags must be an iterable of strings")
    try:
        tags = list(islice(tags, 33))
    except TypeError as error:
        raise ValueError("tags must be an iterable of strings") from error
    if len(tags) > 32:
        raise ValueError("at most 32 tags are supported")
    for tag in tags:
        _field(tag, "tag", 128)
    if len(set(tags)) != len(tags):
        raise ValueError("tags must be distinct")
    text = extract_text(html)
    header = {"type": "vecnook_collection", "schema_version": 1,
              "space": {"dimensions": len(vector), "model": model_identity, "metric": METRICS[metric]}}
    row = {"id": identifier, "source": source, "start_line": 1, "end_line": html.count("\n") + 1,
           "text": text, "tags": tags, "numbers": {}, "dates": {}, "vector": vector}
    lines = [json.dumps(x, ensure_ascii=False, allow_nan=False) for x in (header, row)]
    if (len((lines[0] + "\n").encode("utf-8")) > 64 * 1024
            or len((lines[1] + "\n").encode("utf-8")) > 128 * 1024):
        raise ValueError("JSONL header or row exceeds the existing import limit")
    return "\n".join(lines) + "\n"


def _bounded_text(path, limit):
    with Path(path).open("rb") as source:
        data = source.read(limit + 1)
    if len(data) > limit:
        raise ValueError("Input file exceeds the adapter byte limit")
    return data.decode("utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("html", type=Path)
    parser.add_argument("--vector", type=Path, required=True, help="JSON array produced by the application")
    parser.add_argument("--id", required=True, help="Canonical u64 decimal string")
    parser.add_argument("--model-identity", required=True, help="Exact identity of the application's embedding configuration")
    parser.add_argument("--source", help="Source label; defaults to the input filename")
    parser.add_argument("--metric", choices=tuple(METRICS), default="l2")
    parser.add_argument("--tag", action="append", default=[])
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        html = _bounded_text(args.html, MAX_INPUT_BYTES)
        vector = json.loads(_bounded_text(args.vector, MAX_VECTOR_BYTES))
        result = collection_jsonl(html, identifier=args.id,
                                  source=args.html.name if args.source is None else args.source,
                                  vector=vector, model_identity=args.model_identity,
                                  metric=args.metric, tags=args.tag)
        with args.output.open("x", encoding="utf-8", newline="\n") as output:
            output.write(result)
    except (OSError, UnicodeError, ValueError) as error:
        parser.exit(2, f"html-jsonl: {error}\n")
    print(f"Wrote one JSONL document to {args.output}")


if __name__ == "__main__":
    main()
