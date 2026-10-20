"""Local subprocess client. Install the Vecnook CLI separately; no telemetry.

Methods are blocking. Use a background task in a GUI. Each call opens the
collection; do not use alongside an application that owns its exclusive lock.
"""
import json
import copy
import math
from pathlib import Path
import re
import struct
import subprocess
import tempfile


class VecnookError(RuntimeError):
    """CLI failure with a stable native error kind and original message."""

    def __init__(self, kind, message):
        super().__init__(message)
        self.kind = kind


class Client:
    """Own a collection path and an installed binary, without opening a handle."""

    def __init__(self, path, binary="vecnook", timeout=120):
        self.path, self.binary, self.timeout = Path(path), str(binary), timeout

    def _run(self, command, *arguments, json_result=True):
        result = subprocess.run([self.binary, "--json-errors", command, *map(str, arguments)],
                                capture_output=True, text=True, encoding="utf-8", timeout=self.timeout)
        if result.returncode:
            try:
                error = json.loads(result.stderr)["error"]
            except (ValueError, KeyError):
                raise VecnookError("Transport", result.stderr.strip()) from None
            raise VecnookError(error["kind"], error["message"])
        return json.loads(result.stdout) if json_result else result.stdout

    @classmethod
    def create(cls, path, dimensions, model_identity, metric="cosine", **kwargs):
        """Create a fresh collection; the caller owns embedding generation."""
        client = cls(path, **kwargs)
        client._run("docs-init", client.path.parent, client.path.name, dimensions,
                    model_identity, metric, json_result=False)
        return client

    def info(self):
        """Inspect immutable model/dimension/metric identity."""
        return self._run("docs-info", "--db", self.path)

    def inspect(self):
        """Read original rows/vectors and diagnostics without modifying storage."""
        return self._run("docs-inspect", "--db", self.path)

    def get(self, identifier, include_vector=False):
        """Retrieve one original document, with an optional original vector."""
        flags = ["--vector"] if include_vector else []
        return self._run("docs-get", "--db", self.path, str(identifier), *flags)

    def sync(self, folder, *, dry_run=False, prune=False, model=None, namespace=None):
        """Sync local Markdown using the installed Ollama model identity."""
        flags = ["--dry-run"] if dry_run else []
        if prune:
            flags.append("--prune")
        for name, value in [("--model", model), ("--namespace", namespace)]:
            if value is not None:
                flags.extend([name, value])
        return self._run("docs-sync", folder, "--db", self.path, "--json", *flags)

    def search_keywords(self, text, k=10, *, where=None, analyzer="words"):
        """Lexical search for any embedding space, without model requests."""
        flags = ["--where", where] if where is not None else []
        return self._run("docs-keywords", "--db", self.path, text, k,
                         "--analyzer", analyzer, *flags)

    def search_vector(self, vector, k=10, *, where=None, strategy="auto"):
        """Query caller-provided coordinates from the collection's exact space."""
        flags = ["--where", where] if where is not None else []
        encoded = _coordinates(vector)
        if len(encoded) > 8192:
            with tempfile.TemporaryDirectory(prefix="vecnook-python-query-") as temporary:
                path = Path(temporary)/"vector.csv"
                path.write_text(encoded, encoding="utf-8")
                return self._run("docs-search", "--db", self.path, "@"+str(path), k, max(128,k), strategy, *flags)
        return self._run("docs-search", "--db", self.path, encoded, k,
                         max(128, k), strategy, *flags)

    def search(self, text, k=3, *, mode="auto", analyzer="words", where=None,
               group_by_source=False, context_chunks=0, max_distance=None, reranker=None):
        """Ollama text retrieval. An optional callback reorders bounded candidates.

        A reranker receives (query, neighbors) and returns IDs in a new order.
        It cannot introduce rows or change stored payloads/scores.
        """
        if type(k) is not int or not 1 <= k <= 100:
            raise ValueError("k must be an integer in 1..100")
        flags = ["--mode", mode, "--analyzer", analyzer, "--context-chunks", context_chunks]
        if where is not None:
            flags += ["--where", where]
        if group_by_source:
            flags.append("--group-by-source")
        if max_distance is not None:
            flags += ["--max-distance", max_distance]
        result = self._run("docs-find", text, "--db", self.path, "--k", 100 if reranker else k,
                           "--json", *flags)
        if reranker:
            original = {n["id"]: n for n in result["neighbors"]}
            order = list(reranker(text, copy.deepcopy(list(original.values()))))
            if len(order) != len(original) or set(order) != set(original):
                raise ValueError("reranker must return each original candidate ID exactly once")
            result["candidate_complete"] = result.get("complete")
            result["neighbors"] = [original[i] for i in order[:k]]
            result["reranker_applied"] = True
            if "complete" in result:
                eligible = result.get("eligible_count", result.get("vector", {}).get("eligible_count", 0))
                result["complete"] = len(result["neighbors"]) == min(k, eligible)
        return result

    def put(self, document, vector, *, expected_sequence=None):
        """Atomically put a document and caller-provided f32 vector.

        Document IDs must be canonical decimal strings. Core validation rejects
        invalid payloads before writing. A failed I/O result can be ambiguous;
        inspect affected IDs before retrying.
        """
        identifier = document["id"]
        if not isinstance(identifier, str) or not re.fullmatch(r"0|[1-9][0-9]*", identifier):
            raise ValueError("IDs must be canonical decimal strings")
        with tempfile.TemporaryDirectory(prefix="vecnook-python-") as temporary:
            batch = Path(temporary) / "put.tsv"
            batch.write_text(f"put\t{identifier}\t{_coordinates(vector)}\t{_payload(document)}\n", encoding="utf-8")
            flags = ["--if-sequence", expected_sequence] if expected_sequence is not None else []
            return self._run("docs-batch", "--db", self.path, batch, *flags, json_result=False)

    def put_text(self, document, embed, model_identity, *, expected_sequence=None):
        """Use an application-owned embedding callable; no provider is implicit."""
        if self.info()["model"] != model_identity:
            raise VecnookError("EmbeddingMismatch", "embedding identity differs from collection")
        vectors = embed([document["text"]])
        if len(vectors) != 1:
            raise ValueError("embedding response count mismatch")
        return self.put(document, vectors[0], expected_sequence=expected_sequence)

    def export_jsonl(self, destination):
        """Export original records to a new JSONL archive."""
        return self._run("docs-export-jsonl", "--db", self.path, "--output", destination, json_result=False)

    @classmethod
    def import_jsonl(cls, source, destination, **kwargs):
        """Validate an archive before creating a new collection."""
        client = cls(destination, **kwargs)
        client._run("docs-import-jsonl", source, "--db", destination, json_result=False)
        return client


def _coordinates(vector):
    values = list(vector)
    if not values or any(type(v) not in {int, float} or not math.isfinite(v) or abs(v) > 3.4028234663852886e38 for v in values):
        raise ValueError("coordinates must be finite f32 values")
    return ",".join(format(v, ".9g") for v in values)


def _payload(document):
    def string(value):
        raw = value.encode("utf-8")
        return struct.pack("<I", len(raw))+raw
    numbers, dates = document.get("numbers", {}), document.get("dates", {})
    data = struct.pack("<QII", int(document["id"]), document.get("start_line", 1), document.get("end_line", 1))
    data += string(document["source"])+string(document["text"])
    tags = document.get("tags", [])
    data += struct.pack("<I", len(tags))+b"".join(string(t) for t in tags)
    if numbers or dates:
        data += struct.pack("<I", len(numbers))
        for key, value in sorted(numbers.items()):
            data += string(key)+struct.pack("<d", value)
        data += struct.pack("<I", len(dates))
        for key, value in sorted(dates.items()):
            from datetime import date
            d = date.fromisoformat(value)
            data += string(key)+struct.pack("<I", d.year*10000+d.month*100+d.day)
    return ("VDOC2:" if numbers or dates else "VDOC1:")+data.hex()
