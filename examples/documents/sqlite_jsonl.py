#!/usr/bin/env python3
"""Read a local SQLite SELECT into JSONL v1 using caller-owned vectors (Python 3.11+)."""
import argparse
import json
from pathlib import Path
import re
import sqlite3
import time

import search

COLUMNS = ("id", "source", "start_line", "end_line", "text", "vector_json",
           "tags_json", "numbers_json", "dates_json")
METRICS = {"l2": "squared_l2", "ip": "negative_inner_product", "cosine": "cosine"}
MAX_SQL_BYTES = 8192
MAX_SQLITE_ROW_BYTES = 128 * 1024
MAX_ROWS = 10_000
MAX_OUTPUT_BYTES = 32 * 1024 * 1024
MAX_VM_STEPS = 1_000_000
MAX_QUERY_SECONDS = 5
F32_OVERFLOW = float.fromhex("0x1.ffffffp+127")


def _field(value, label, limit):
    if not isinstance(value, str) or not value or "\0" in value:
        raise ValueError(f"{label} must be a nonempty string without NUL")
    if len(value.encode("utf-8")) > limit:
        raise ValueError(f"{label} exceeds {limit} UTF-8 bytes")


def _json(value, label):
    if not isinstance(value, str):
        raise ValueError(f"{label} must be JSON text")
    try:
        return json.loads(value, parse_int=lambda raw: -0.0 if raw == "-0" else int(raw))
    except (ValueError, RecursionError) as error:
        raise ValueError(f"{label} is invalid JSON") from error


def _document(row, dimensions, metric):
    identifier = row["id"]
    if type(identifier) is int:
        identifier = str(identifier)
    if (not isinstance(identifier, str) or len(identifier) > 20
            or re.fullmatch(r"0|[1-9][0-9]*", identifier) is None
            or int(identifier) > 2**64 - 1):
        raise ValueError("id must be a canonical u64 decimal string or nonnegative SQLite integer")
    _field(row["source"], "source", 1024)
    if not isinstance(row["text"], str):
        raise ValueError("text must be UTF-8 text")
    if (type(row["start_line"]) is not int or type(row["end_line"]) is not int
            or not 1 <= row["start_line"] <= row["end_line"] <= 2**32 - 1):
        raise ValueError("invalid source line range")
    vector = _json(row["vector_json"], "vector_json")
    if (not isinstance(vector, list) or len(vector) != dimensions
            or any(type(v) not in (int, float) or not -F32_OVERFLOW < v < F32_OVERFLOW
                   for v in vector)
            or (metric == "cosine" and not any(abs(v) > 2**-150 for v in vector))):
        raise ValueError("vector dimensions or finite f32 coordinates are invalid")
    tags = _json(row["tags_json"], "tags_json")
    numbers = _json(row["numbers_json"], "numbers_json")
    dates = _json(row["dates_json"], "dates_json")
    if (not isinstance(tags, list) or any(not isinstance(tag, str) for tag in tags)
            or not isinstance(numbers, dict) or not isinstance(dates, dict)
            or any(type(v) not in (int, float) for v in numbers.values())
            or any(not isinstance(v, str) for v in dates.values())):
        raise ValueError("invalid existing JSONL tags or typed attributes")
    chunk = search.Chunk(row["source"], row["start_line"], row["end_line"], row["text"])
    search.encode_document(int(identifier), chunk, tags, numbers, dates)
    return dict(id=identifier, source=chunk.source, start_line=chunk.start_line,
                end_line=chunk.end_line, text=chunk.text, vector=vector,
                tags=tags, numbers=numbers, dates=dates)


def _authorize(action, first, second, database, _origin):
    if action == sqlite3.SQLITE_READ:
        return sqlite3.SQLITE_OK if database == "main" else sqlite3.SQLITE_DENY
    if action == sqlite3.SQLITE_FUNCTION:
        name = (second or "").lower()
        return (sqlite3.SQLITE_DENY if name in {"load_extension", "readfile", "writefile"}
                else sqlite3.SQLITE_OK)
    return (sqlite3.SQLITE_OK if action in {sqlite3.SQLITE_SELECT, sqlite3.SQLITE_RECURSIVE}
            else sqlite3.SQLITE_DENY)


def _rows(database, query):
    """Yield one SELECT's rows from a read transaction; callers must close the generator."""
    if not hasattr(sqlite3.Connection, "setlimit") or sqlite3.sqlite_version_info < (3, 22, 0):
        raise ValueError("This optional example needs Python 3.11+ with SQLite 3.22+")
    _field(query, "SQL query", MAX_SQL_BYTES)
    path = Path(database).resolve(strict=True)
    if not path.is_file():
        raise ValueError("database must be an existing local file")
    con = sqlite3.connect(path.as_uri() + "?mode=ro", uri=True, timeout=1, isolation_level=None)
    try:
        con.setlimit(sqlite3.SQLITE_LIMIT_LENGTH, MAX_SQLITE_ROW_BYTES)
        con.setlimit(sqlite3.SQLITE_LIMIT_SQL_LENGTH, MAX_SQL_BYTES)
        con.setlimit(sqlite3.SQLITE_LIMIT_ATTACHED, 0)
        con.execute("PRAGMA query_only=ON")
        if con.execute("PRAGMA query_only").fetchone() != (1,):
            raise ValueError("SQLite query-only mode is unavailable")
        con.execute("BEGIN")
        con.set_authorizer(_authorize)
        start, steps = time.monotonic(), 0
        def progress():
            nonlocal steps
            steps += 1000
            return int(steps > MAX_VM_STEPS or time.monotonic() - start > MAX_QUERY_SECONDS)
        con.set_progress_handler(progress, 1000)
        cursor = con.execute(query)
        if cursor.description is None or tuple(c[0] for c in cursor.description) != COLUMNS:
            raise ValueError("SELECT columns must be exactly: " + ", ".join(COLUMNS))
        for index, values in enumerate(cursor):
            if index >= MAX_ROWS:
                raise ValueError(f"SELECT exceeds {MAX_ROWS} documents")
            if time.monotonic() - start > MAX_QUERY_SECONDS:
                raise ValueError("SELECT exceeds the cooperative query-time limit")
            yield dict(zip(COLUMNS, values))
    finally:
        con.close()


def export_sqlite(database, query, output, *, model_identity, dimensions, metric="l2"):
    """Validate the complete bounded export before exclusively creating a JSONL file."""
    _field(model_identity, "model identity", 512)
    if type(dimensions) is not int or not 1 <= dimensions <= 4096 or not isinstance(metric, str) or metric not in METRICS:
        raise ValueError("invalid dimensions or metric")
    header = dict(type="vecnook_collection", schema_version=1,
                  space=dict(dimensions=dimensions, model=model_identity, metric=METRICS[metric]))
    lines = [json.dumps(header, ensure_ascii=False, allow_nan=False) + "\n"]
    total, seen = len(lines[0].encode("utf-8")), set()
    iterator = _rows(database, query)
    try:
        for row in iterator:
            document = _document(row, dimensions, metric)
            if document["id"] in seen:
                raise ValueError("SELECT returned a duplicate document id")
            seen.add(document["id"])
            line = json.dumps(document, ensure_ascii=False, allow_nan=False) + "\n"
            if len(line.encode("utf-8")) > 128 * 1024:
                raise ValueError("JSONL document exceeds 128 KiB")
            total += len(line.encode("utf-8"))
            if total > MAX_OUTPUT_BYTES:
                raise ValueError("JSONL export exceeds 32 MiB")
            lines.append(line)
    finally:
        iterator.close()
    with Path(output).open("x", encoding="utf-8", newline="\n") as target:
        target.writelines(lines)
    return len(seen)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("database", type=Path)
    parser.add_argument("--sql", type=Path, required=True, help="UTF-8 SELECT file with the documented aliases")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--model-identity", required=True)
    parser.add_argument("--dimensions", type=int, required=True)
    parser.add_argument("--metric", choices=tuple(METRICS), default="l2")
    args = parser.parse_args()
    try:
        with args.sql.open("rb") as source:
            raw = source.read(MAX_SQL_BYTES + 1)
        if len(raw) > MAX_SQL_BYTES:
            raise ValueError("SQL file exceeds 8192 bytes")
        count = export_sqlite(args.database, raw.decode("utf-8"), args.output,
                              model_identity=args.model_identity, dimensions=args.dimensions, metric=args.metric)
    except (OSError, UnicodeError, ValueError, OverflowError, sqlite3.Error) as error:
        parser.exit(2, f"sqlite-jsonl: {error}\n")
    print(f"Wrote {count} JSONL documents to {args.output}")


if __name__ == "__main__":
    main()
