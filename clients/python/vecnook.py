"""Local subprocess client. Install the Vecnook CLI separately; no telemetry.

Methods are blocking. Use a background task in a GUI. Client calls open the
collection; do not use alongside an application that owns its exclusive lock.
SnapshotSession instead retains one read-only published generation in a child
process until explicitly closed. It does not open a mutable collection.
"""
import json
import copy
import math
import queue
from pathlib import Path
import re
import struct
import subprocess
import tempfile
import threading
import time
import weakref


class VecnookError(RuntimeError):
    """CLI failure with a stable native error kind and original message."""

    def __init__(self, kind, message):
        super().__init__(message)
        self.kind = kind


class SnapshotSession:
    """Reuse one native JSONL snapshot reader, optionally with prepared SQ8.

    Use a context manager or call close(). Methods serialize on this instance
    and block until their I/O deadline. A timeout or broken protocol terminates
    the reader; it never silently reopens a newer generation. info() returns
    the original ready event, including separate open/build/cache timings.
    """

    def __init__(self, path, *, binary="vecnook", timeout=120, sq8=None,
                 sq8_cache=None, buffer_bytes=None, idle_ms=60000,
                 max_response_bytes=16 * 1024 * 1024):
        if (type(timeout) not in {int, float} or not 0 < timeout <= 86400
                or not math.isfinite(timeout)):
            raise ValueError("timeout must be positive finite seconds, at most 86400")
        for name, value, upper in [("idle_ms", idle_ms, 86400000),
                                    ("max_response_bytes", max_response_bytes, 64 * 1024 * 1024)]:
            if type(value) is not int or not 1 <= value <= upper:
                raise ValueError(f"{name} must be an integer in 1..{upper}")
        if sq8 is not None and (type(sq8) is not int or not 1 <= sq8 <= 65536):
            raise ValueError("sq8 must be an integer in 1..65536")
        if sq8_cache is not None and sq8 is None:
            raise ValueError("sq8_cache requires sq8")
        if buffer_bytes is not None and (sq8_cache is None or type(buffer_bytes) is not int
                                         or not 1 <= buffer_bytes <= 16 * 1024 * 1024):
            raise ValueError("buffer_bytes requires a cache and must be in 1..16777216")
        command = [str(binary), "--json-errors", "snapshot-session", str(path),
                   "--idle-ms", str(idle_ms)]
        for flag, value in [("--sq8", sq8), ("--sq8-cache", sq8_cache),
                            ("--buffer-bytes", buffer_bytes)]:
            if value is not None:
                command.extend([flag, str(value)])
        self.timeout = timeout
        self._lock = threading.Lock()
        self._closed = False
        self._next_id = 0
        self._jobs, self._responses = queue.Queue(1), queue.Queue(1)
        self._stderr = bytearray()
        try:
            self._process = subprocess.Popen(command, stdin=subprocess.PIPE,
                                             stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        except OSError as error:
            raise VecnookError("Transport", str(error)) from error
        worker = threading.Thread(target=_snapshot_io,
                                  args=(self._process, self._jobs, self._responses,
                                        max_response_bytes), daemon=True)
        errors = threading.Thread(target=_snapshot_stderr,
                                  args=(self._process.stderr, self._stderr), daemon=True)
        self._cleanup = weakref.finalize(self, _snapshot_cleanup, self._process,
                                         self._jobs, worker, errors)
        worker.start()
        errors.start()
        self._ready = self._receive(time.monotonic() + timeout)
        if (self._ready.get("event") != "ready"
                or any(not isinstance(self._ready.get(k), str)
                       or len(self._ready[k]) > 20
                       or not re.fullmatch(r"0|[1-9][0-9]*", self._ready[k])
                       or int(self._ready[k]) > 2**64 - 1
                       for k in ("generation", "sequence"))):
            self._fail("snapshot reader did not send a ready event")

    @property
    def closed(self):
        """Whether this reader has closed or its native process has exited."""
        return self._closed or self._process.poll() is not None

    def info(self):
        """Return a copy of the initial pinned-generation/constructor diagnostics."""
        return copy.deepcopy(self._ready)

    def search_vector(self, vector, k=10, *, ids=None):
        """Return the native response envelope and original generation-search result.

        ids is an optional bounded eligibility list of canonical u64 strings.
        It is not an authorization policy. No embedding provider is invoked.
        """
        if type(k) is not int or not 1 <= k <= 10000:
            raise ValueError("k must be an integer in 1..10000")
        values = []
        for value in vector:
            if len(values) == 4096:
                raise ValueError("vectors must have at most 4096 coordinates")
            if (type(value) not in {int, float} or abs(value) > 3.4028234663852886e38
                    or not math.isfinite(value)):
                raise ValueError("coordinates must be finite f32 values")
            values.append(value)
        if not values:
            raise ValueError("vectors must not be empty")
        payload = {"vector": values, "k": k}
        if ids is not None:
            payload["ids"] = _snapshot_ids(ids)
        with self._lock:
            response = self._exchange(payload, time.monotonic() + self.timeout)
            result = response.get("result")
            if (not isinstance(result, dict)
                    or result.get("generation") != self._ready.get("generation")
                    or result.get("sequence") != self._ready.get("sequence")):
                self._fail("snapshot response changed its pinned generation or sequence")
            return response

    def search_batch(self, vectors, k=10, *, ids=None):
        """Yield one response at a time using this reader, without buffering a batch.

        Results already yielded remain valid if a later query fails. The input
        iterator and results are caller-owned; this is not an atomic operation.
        """
        eligible = _snapshot_ids(ids) if ids is not None else None
        for vector in vectors:
            yield self.search_vector(vector, k, ids=eligible)

    def close(self):
        """Close normally when possible, then reap the child and release its leases."""
        with self._lock:
            if not self._closed and self._process.poll() is None:
                try:
                    reply = self._exchange({"op": "close"},
                                           time.monotonic() + min(self.timeout, 5), closing=True)
                    if reply.get("event") != "closed":
                        self._fail("snapshot reader did not acknowledge close")
                    self._process.wait(timeout=min(self.timeout, 5))
                except (VecnookError, subprocess.TimeoutExpired):
                    pass
            self._closed = True
            self._cleanup()

    def __enter__(self):
        if self.closed:
            raise VecnookError("Transport", "snapshot session is closed")
        return self

    def __exit__(self, *_):
        self.close()

    def _exchange(self, payload, deadline, closing=False):
        if self.closed:
            self._fail("snapshot session is closed")
        self._next_id += 1
        request_id = str(self._next_id)
        payload = {"request_id": request_id, **payload}
        encoded = json.dumps(payload, ensure_ascii=False, allow_nan=False,
                             separators=(",", ":")).encode("utf-8") + b"\n"
        if len(encoded) > 1024 * 1024:
            raise ValueError("snapshot request exceeds 1 MiB")
        try:
            self._jobs.put(encoded, timeout=max(0, deadline - time.monotonic()))
        except queue.Full:
            self._fail("snapshot request timed out while writing")
        response = self._receive(deadline)
        if response.get("event") in {"closed", "idle_timeout"} and not closing:
            self._fail("snapshot session ended: " + response["event"])
        if response.get("request_id") != request_id:
            self._fail("snapshot response request ID differs from the request")
        if "error" in response:
            error = response["error"]
            if not isinstance(error, dict) or not all(isinstance(error.get(k), str)
                                                      for k in ("kind", "message")):
                self._fail("snapshot reader returned an invalid error")
            raise VecnookError(error["kind"], error["message"])
        return response

    def _receive(self, deadline):
        try:
            raw = self._responses.get(timeout=max(0, deadline - time.monotonic()))
        except queue.Empty:
            self._fail("snapshot request timed out")
        if isinstance(raw, Exception) or not raw:
            self._fail("snapshot reader stopped before completing its response")
        try:
            response = json.loads(raw.decode("utf-8"), parse_constant=_snapshot_nonfinite,
                                  parse_float=_snapshot_float, object_pairs_hook=_snapshot_object)
        except (ValueError, UnicodeError):
            self._fail("snapshot reader returned invalid UTF-8 JSON")
        if not isinstance(response, dict) or type(response.get("schema_version")) is not int or response["schema_version"] != 1:
            self._fail("snapshot reader returned an unsupported JSON schema")
        return response

    def _fail(self, message):
        self._closed = True
        self._cleanup()
        try:
            error = json.loads(self._stderr.decode("utf-8"))["error"]
            if isinstance(error["kind"], str) and isinstance(error["message"], str):
                raise VecnookError(error["kind"], error["message"])
        except (ValueError, UnicodeError, KeyError, TypeError):
            pass
        raise VecnookError("Transport", message)


def _snapshot_nonfinite(value):
    raise ValueError("non-finite JSON number: " + value)


def _snapshot_float(value):
    number = float(value)
    if not math.isfinite(number):
        _snapshot_nonfinite(value)
    return number


def _snapshot_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON field: " + key)
        result[key] = value
    return result


def _snapshot_ids(ids):
    if isinstance(ids, (str, bytes)):
        raise ValueError("ids must be an iterable of canonical u64 strings")
    values = []
    for identifier in ids:
        if len(values) == 65536:
            raise ValueError("ids must contain at most 65536 entries")
        if (not isinstance(identifier, str) or len(identifier) > 20
                or not re.fullmatch(r"0|[1-9][0-9]*", identifier) or int(identifier) > 2**64 - 1):
            raise ValueError("IDs must be canonical u64 decimal strings")
        values.append(identifier)
    return values


def _snapshot_io(process, jobs, responses, limit):
    try:
        while True:
            frame = process.stdout.readline(limit + 1)
            if len(frame) > limit or (frame and not frame.endswith(b"\n")):
                raise ValueError("snapshot response exceeds its byte limit or is incomplete")
            responses.put_nowait(frame)
            if not frame:
                return
            request = jobs.get()
            if request is None:
                return
            process.stdin.write(request)
            process.stdin.flush()
    except (OSError, ValueError, queue.Full) as error:
        try:
            responses.put_nowait(error)
        except queue.Full:
            pass


def _snapshot_stderr(stream, buffer):
    try:
        while chunk := stream.read(4096):
            buffer.extend(chunk)
            if len(buffer) > 65536:
                del buffer[:-65536]
    except (OSError, ValueError):
        pass


def _snapshot_cleanup(process, jobs, worker, errors):
    if process.poll() is None:
        process.kill()
    process.wait(timeout=5)
    try:
        jobs.put_nowait(None)
    except queue.Full:
        pass
    worker.join(timeout=1)
    errors.join(timeout=1)
    for stream in (process.stdin, process.stdout, process.stderr):
        try:
            stream.close()
        except OSError:
            pass


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
