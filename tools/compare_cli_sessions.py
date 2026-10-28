#!/usr/bin/env python3
"""Measure actual generation CLI lifetimes; preserve first-use and repeated costs."""

import argparse
import concurrent.futures
import datetime
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import queue
import re
import struct
import subprocess
import threading
import time


def sha(path):
    h = hashlib.sha256()
    with open(path, "rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def fixture_inventory(fixture):
    rows = [{"path": str(path.relative_to(fixture)), "bytes": path.stat().st_size, "sha256": sha(path)}
            for path in sorted(fixture.rglob("*")) if path.is_file()]
    return {"files": rows, "sha256": hashlib.sha256(json.dumps(rows, sort_keys=True).encode()).hexdigest()}


def percentile(values, fraction=0.95):
    values = sorted(values)
    return values[round((len(values) - 1) * fraction)]


def vectors(path):
    result = []
    with open(path, "rb") as source:
        while header := source.read(4):
            if len(header) != 4:
                raise ValueError("truncated fvecs header")
            dimensions = struct.unpack("<I", header)[0]
            if not 1 <= dimensions <= 4096:
                raise ValueError("invalid fvecs dimensions")
            data = source.read(4 * dimensions)
            if len(data) != 4 * dimensions:
                raise ValueError("truncated fvecs vector")
            vector = list(struct.unpack(f"<{dimensions}f", data))
            if not all(math.isfinite(v) for v in vector):
                raise ValueError("non-finite query")
            result.append(vector)
            if len(result) == 120:
                break
    if len(result) < 21:
        raise ValueError("need 20 calibration queries and disjoint validation queries")
    return result[20:]


def canonical(report):
    """Compare identities and f64 bits, never accept a recall-only approximation."""
    return {
        key: report[key]
        for key in ("generation", "sequence", "approximate", "complete", "eligible_count", "original_distance_computations")
    } | {"neighbors": [
        [n["id"], struct.pack("<d", n["distance"]).hex(), n.get("metadata")]
        for n in report["neighbors"]
    ]}


def decode(text):
    # JSON permits -0 as an integer token; preserve its sign as an f64 distance.
    return json.loads(text, parse_int=lambda token: -0.0 if token == "-0" else int(token))


def native_rss(stderr):
    if platform.system() == "Darwin":
        match = re.search(r"(\d+)\s+maximum resident set size", stderr)
        return int(match[1]) if match else None
    match = re.search(r"Maximum resident set size \(kbytes\):\s*(\d+)", stderr)
    return int(match[1]) * 1024 if match else None


def timed(command):
    flag = "-l" if platform.system() == "Darwin" else "-v"
    return ["/usr/bin/time", flag, *map(str, command)]


def validate_ready(ready, cached):
    if ready.get("event") != "ready" or ready.get("snapshot_opens") != 1:
        raise ValueError("missing initialization evidence / not a retained session")
    if ready.get("sq8_builds") != (0 if cached else 1):
        raise ValueError("unexpected SQ8 build count")


def once(binary, fixture, query_file, budget, log, extra=()):
    start = time.perf_counter()
    result = subprocess.run(timed([binary, "snapshot-search", fixture / "store", "@" + str(query_file), "10", "--sq8", str(budget), *extra]),
                            capture_output=True, text=True, timeout=60, check=False)
    elapsed = (time.perf_counter() - start) * 1000
    log.with_suffix(".stdout").write_text(result.stdout)
    log.with_suffix(".stderr").write_text(result.stderr)
    if result.returncode:
        raise RuntimeError(f"CLI failed; see {log}")
    return {"end_to_end_ms": elapsed, "peak_rss_bytes": native_rss(result.stderr),
            "result": canonical(decode(result.stdout)), "process_starts": 1,
            "snapshot_opens": 1, "sq8_builds": 0 if extra else 1,
            "initialization_evidence": "one fresh CLI process per query; existing command control flow"}


class GroupSampler:
    """Sample engine process groups; sampled RSS is not a native high-water mark."""
    def __init__(self):
        self.roots = set()
        self.rows = []
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self.collect, daemon=True)

    def collect(self):
        while not self.stop.is_set():
            try:
                ps = subprocess.run(["/bin/ps", "-axo", "pid=,ppid=,rss="], capture_output=True, text=True, timeout=3)
                rows = [tuple(map(int, line.split())) for line in ps.stdout.splitlines() if len(line.split()) == 3]
                included = self.roots.copy()
                for _ in range(5):
                    included |= {pid for pid, parent, _ in rows if parent in included}
                selected = [(pid, rss * 1024) for pid, _, rss in rows if pid in included and pid not in self.roots]
                sample = {"monotonic": time.monotonic(), "engine_pids": [pid for pid, _ in selected],
                          "sampled_group_rss_bytes": sum(rss for _, rss in selected),
                          "excludes": "time wrappers, driver, editor and OS filesystem cache"}
                if platform.system() == "Linux":
                    pss, private, shared = 0, 0, 0
                    for pid, _ in selected:
                        try:
                            data = Path(f"/proc/{pid}/smaps_rollup").read_text()
                            fields = {m[0]: int(m[1]) * 1024 for m in re.findall(r"^(\w+):\s+(\d+) kB", data, re.M)}
                            pss += fields.get("Pss", 0)
                            private += fields.get("Private_Clean", 0) + fields.get("Private_Dirty", 0)
                            shared += fields.get("Shared_Clean", 0) + fields.get("Shared_Dirty", 0)
                        except (OSError, ValueError):
                            sample["linux_partial"] = True
                    sample.update(pss_bytes=pss, private_bytes=private, shared_bytes=shared)
                self.rows.append(sample)
            except (OSError, ValueError, subprocess.TimeoutExpired):
                pass
            self.stop.wait(0.1)

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *_):
        self.stop.set()
        self.thread.join(timeout=5)


def session(binary, fixture, queries, budget, log, cache=None, group=None, buffer_bytes=None):
    args = [binary, "snapshot-session", fixture / "store", "--sq8", str(budget), "--idle-ms", "30000"]
    if cache:
        args += ["--sq8-cache", cache]
        if buffer_bytes is not None:
            args += ["--buffer-bytes", str(buffer_bytes)]
    stderr = log.with_suffix(".stderr").open("w")
    start = time.perf_counter()
    child = subprocess.Popen(timed(args), stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stderr, text=True, bufsize=1)
    if group is not None:
        group.roots.add(child.pid)
    output = queue.Queue(maxsize=2)

    def read():
        try:
            for line in child.stdout:
                output.put(line)
        finally:
            output.put(None)

    threading.Thread(target=read, daemon=True).start()
    transcript, rows = [], []

    def receive():
        line = output.get(timeout=60)
        if line is None:
            raise RuntimeError(f"premature session EOF; see {log}")
        transcript.append(line)
        return decode(line)

    try:
        ready = receive()
        startup_ms = (time.perf_counter() - start) * 1000
        validate_ready(ready, cache is not None)
        for i, vector in enumerate(queries):
            start = time.perf_counter()
            child.stdin.write(json.dumps({"request_id": str(i), "vector": vector, "k": 10}) + "\n")
            child.stdin.flush()
            response = receive()
            elapsed = (time.perf_counter() - start) * 1000
            if response.get("request_id") != str(i) or "error" in response:
                raise ValueError(f"request failed: {response}")
            if response.get("snapshot_opens") != 1 or response.get("sq8_builds") != ready["sq8_builds"]:
                raise ValueError("repeated initialization detected")
            rows.append({"end_to_end_ms": elapsed, "engine_search_ms": response["search_ms"], "result": canonical(response["result"])})
        child.stdin.write('{"request_id":"close","op":"close"}\n')
        child.stdin.flush()
        if receive().get("event") != "closed":
            raise ValueError("missing close acknowledgement")
        child.stdin.close()
        if child.wait(timeout=10):
            raise RuntimeError(f"session failed; see {log}")
    finally:
        if child.poll() is None:
            child.kill()
            child.wait(timeout=10)
        stderr.close()
        log.with_suffix(".stdout").write_text("".join(transcript))
    return {"startup_ms": startup_ms, "first_query_ms": rows[0]["end_to_end_ms"],
            "repeated_p95_ms": percentile([r["end_to_end_ms"] for r in rows[1:]]),
            "all_query_p95_ms": percentile([r["end_to_end_ms"] for r in rows]),
            "engine_search_p95_ms": percentile([r["engine_search_ms"] for r in rows]),
            "peak_rss_bytes": native_rss(log.with_suffix(".stderr").read_text()),
            "snapshot_opens": ready["snapshot_opens"], "sq8_builds": ready["sq8_builds"],
            "startup": ready, "queries": rows}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline-binary", required=True)
    parser.add_argument("--candidate-binary")
    parser.add_argument("--fixture", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--baseline-results")
    parser.add_argument("--baseline-only", action="store_true")
    parser.add_argument("--resident-only", action="store_true", help="validate P0 before cache support")
    parser.add_argument("--budget", type=int, default=64)
    parser.add_argument("--repetitions", type=int, default=3)
    parser.add_argument("--query-count", type=int, default=100)
    parser.add_argument("--readers", default="1,2,4")
    parser.add_argument("--buffer-bytes", type=int, help="explicit cached-session code workspace")
    args = parser.parse_args()
    if platform.system() not in {"Darwin", "Linux"}:
        parser.error("native high-water collection currently supports Darwin/Linux; do not infer Windows performance")
    if not 1 <= args.query_count <= 100 or not 1 <= args.repetitions <= 10:
        parser.error("query-count must be1..100 and repetitions1..10")
    fixture, root = Path(args.fixture).resolve(), Path(args.output).resolve()
    binary = Path(args.baseline_binary).resolve()
    root.mkdir(parents=True, exist_ok=False)
    queries = vectors(fixture / "queries.fvecs")[:args.query_count]
    query_files = []
    for i, vector in enumerate(queries):
        path = root / f"query-{i}.txt"
        path.write_text(",".join(map(repr, vector)))
        query_files.append(path)
    report = {"schema_version": 1, "completed": False, "started_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "platform": platform.platform(), "baseline_binary_sha256": sha(binary), "query_sha256": sha(fixture / "queries.fvecs"),
              "fixture": str(fixture), "budget": args.budget, "query_count": len(queries), "repetitions": args.repetitions,
              "conditions": "warm filesystem; fresh CLI/session processes; input embedding already prepared; IPC serialization included; driver/editor/system activity not isolated; no Rust builds/models/soaks run during samples",
              "baseline": [], "candidate": [], "memory_metrics": "native full-lifetime peak RSS per process; sampled simultaneous groups separately; PSS/private/shared Linux only; missing metrics are unavailable, never zero"}
    report["requested_buffer_bytes"] = args.buffer_bytes
    report["fixture_inventory_before"] = fixture_inventory(fixture)

    def save():
        (root / "summary.json").write_text(json.dumps(report, indent=2) + "\n")

    save()
    if args.baseline_results:
        old = json.loads(Path(args.baseline_results).read_text())
        for field in ("baseline_binary_sha256", "query_sha256", "fixture", "budget", "query_count", "repetitions"):
            if old[field] != report[field]:
                raise ValueError(f"baseline evidence identity mismatch: {field}")
        if not old["completed"]:
            raise ValueError("partial baseline is not reusable")
        report["baseline"] = old["baseline"]
        report["baseline_evidence"] = {"path": str(Path(args.baseline_results).resolve()), "sha256": sha(args.baseline_results)}
    else:
        for repeat in range(args.repetitions):
            rows = []
            for i, path in enumerate(query_files):
                rows.append(once(binary, fixture, path, args.budget, root / f"baseline-{repeat}-{i}"))
                if (i + 1) % 10 == 0:
                    print(json.dumps({"stage": "baseline", "repeat": repeat + 1, "queries": i + 1}), flush=True)
            report["baseline"].append({"queries": rows, "p95_ms": percentile([r["end_to_end_ms"] for r in rows]),
                                       "process_starts": len(rows), "snapshot_opens": len(rows), "sq8_builds": len(rows),
                                       "peak_rss_bytes": max(r["peak_rss_bytes"] for r in rows)})
            save()
    expected = [r["result"] for r in report["baseline"][0]["queries"]]
    if any([r["result"] for r in sample["queries"]] != expected for sample in report["baseline"]):
        raise ValueError("baseline result identity changed")
    if not args.baseline_only:
        if not args.candidate_binary:
            parser.error("candidate-binary required unless baseline-only")
        candidate = Path(args.candidate_binary).resolve()
        report["candidate_binary_sha256"] = sha(candidate)
        for repeat in range(args.repetitions):
            cache = root / f"cache-{repeat}"
            if not args.resident_only:
                start = time.perf_counter()
                preparation = subprocess.run(timed([candidate, "snapshot-prepare-sq8", fixture / "store", cache]), capture_output=True, text=True, timeout=120)
                elapsed = (time.perf_counter() - start) * 1000
                (root / f"prepare-{repeat}.stdout").write_text(preparation.stdout)
                (root / f"prepare-{repeat}.stderr").write_text(preparation.stderr)
                if preparation.returncode:
                    raise RuntimeError("cache preparation failed")
                report.setdefault("preparation", []).append({"result": json.loads(preparation.stdout), "end_to_end_ms": elapsed, "peak_rss_bytes": native_rss(preparation.stderr)})
                extra = ["--sq8-cache", str(cache)]
                if args.buffer_bytes is not None:
                    extra += ["--buffer-bytes", str(args.buffer_bytes)]
                first = once(candidate, fixture, query_files[0], args.budget, root / f"cached-oneshot-{repeat}", extra)
                if first["result"] != expected[0]:
                    raise ValueError("cached first CLI response differs from baseline")
                report.setdefault("cached_oneshot_first_samples", []).append(first)
            for readers in map(int, args.readers.split(",")):
                if not 1 <= readers <= 4:
                    raise ValueError("readers must be1..4")
                for backend, selected_cache in ([("resident", None)] if args.resident_only else [("resident", None), ("cached", cache)]):
                    with GroupSampler() as group:
                        with concurrent.futures.ThreadPoolExecutor(max_workers=readers) as pool:
                            samples = list(pool.map(lambda n: session(candidate, fixture, queries, args.budget,
                                                                     root / f"session-{repeat}-{readers}-{backend}-{n}", selected_cache, group, args.buffer_bytes), range(readers)))
                    for sample in samples:
                        if [r["result"] for r in sample["queries"]] != expected:
                            raise ValueError("candidate differs from exact baseline response bits")
                    report["candidate"].append({"repeat": repeat + 1, "readers": readers, "backend": backend,
                                                "samples": samples, "group_memory": group.rows})
                    save()
                    print(json.dumps({"stage": backend, "repeat": repeat + 1, "readers": readers,
                                      "p95_ms": [s["all_query_p95_ms"] for s in samples],
                                      "peak_rss_bytes": [s["peak_rss_bytes"] for s in samples]}), flush=True)
    report["fixture_inventory_after"] = fixture_inventory(fixture)
    if report["fixture_inventory_before"] != report["fixture_inventory_after"]:
        raise ValueError("fixture changed during measurement")
    report["completed"] = True
    report["finished_utc"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    save()
    print(json.dumps({"completed": True, "summary": str(root / "summary.json"), "baseline_p95_ms": [r["p95_ms"] for r in report["baseline"]]}), flush=True)


if __name__ == "__main__":
    main()
