#!/usr/bin/env python3
"""Verify retained cached CLI sessions from an exact archive for an actual duration."""
import argparse
import datetime
import json
import math
import os
from pathlib import Path
import queue
import signal
import subprocess
import tarfile
import threading
import time

from check_distribution import digest, fingerprint
from check_package import check
from compare_cli_sessions import canonical, decode, fixture_inventory, validate_ready, vectors


def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def write_json(path, value):
    pending = path.with_suffix(".pending")
    pending.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")
    os.replace(pending, path)


def duration_gates(progress):
    counters = ("verified_requests", "reference_queries", "session_starts", "released_cache_leases",
                "session_sq8_builds", "normal_closes", "eof_closes", "forced_kills", "idle_exits",
                "invalid_requests_recovered", "active_lease_refusals", "cache_preparations")
    elapsed = progress.get("elapsed_seconds")
    if (type(elapsed) not in (int, float) or not math.isfinite(elapsed)
            or any(type(progress.get(key)) is not int or progress[key] < 0 for key in counters)):
        return False
    return (elapsed >= 86400
            and progress.get("verified_requests", 0) >= 100000
            and progress.get("reference_queries", 0) == 100
            and all(progress.get(key, 0) >= 1 for key in
                    ("normal_closes", "eof_closes", "forced_kills", "idle_exits"))
            and progress.get("released_cache_leases") == progress.get("session_starts")
            and progress.get("session_starts", 0) >= 4
             and sum(progress[key] for key in ("normal_closes", "eof_closes", "forced_kills", "idle_exits")) == progress["session_starts"]
             and progress["invalid_requests_recovered"] >= 2 * progress["session_starts"]
             and progress["active_lease_refusals"] == progress["session_starts"]
             and progress["cache_preparations"] == progress["session_starts"]
            and progress.get("session_sq8_builds") == 0)


class Session:
    def __init__(self, binary, store, cache, budget, idle_ms, stderr):
        self.child = subprocess.Popen(
            [str(binary), "snapshot-session", str(store), "--sq8", str(budget),
             "--sq8-cache", str(cache), "--idle-ms", str(idle_ms)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stderr,
            text=True, encoding="utf-8", bufsize=1)
        self.events = queue.Queue(maxsize=2)
        self.stopping = threading.Event()

        def offer(value):
            while not self.stopping.is_set():
                try:
                    self.events.put(value, timeout=0.5)
                    return
                except queue.Full:
                    continue
        def read():
            try:
                for line in self.child.stdout:
                    offer(line)
                    if self.stopping.is_set():
                        break
            finally:
                offer(None)
        self.reader = threading.Thread(target=read, daemon=True)
        self.reader.start()
        try:
            ready = self.receive()
            validate_ready(ready, True)
            if ready.get("cache_hits") != 1 or not 0 <= ready["code_read_buffer_bytes"] <= 1048576:
                raise ValueError("expected the final bounded cached backend")
            self.ready = ready
        except BaseException:
            self.stop()
            raise

    def receive(self):
        line = self.events.get(timeout=60)
        if line is None:
            raise RuntimeError("unexpected session EOF")
        response = decode(line)
        if response.get("schema_version") != 1:
            raise ValueError("session schema changed")
        return response

    def send(self, line):
        self.child.stdin.write(line + "\n")
        self.child.stdin.flush()

    def request(self, request_id, vector, expected):
        self.send(json.dumps(dict(request_id=str(request_id), vector=vector, k=10)))
        response = self.receive()
        if (response.get("request_id") != str(request_id)
                or response.get("snapshot_opens") != 1 or response.get("sq8_builds") != 0
                or canonical(response["result"]) != expected):
            raise ValueError("cached session changed result bits, ID, payload or initialization")

    def invalid_then_recover(self):
        self.send('{"request_id":"bad","vector":[NaN],"k":10}')
        if "error" not in self.receive():
            raise ValueError("invalid request was accepted")
        self.send(" " * (1048576 + 1))
        if "error" not in self.receive():
            raise ValueError("oversized request was accepted")

    def finish(self, mode):
        if mode == "normal_closes":
            self.send('{"request_id":"close","op":"close"}')
            event = self.receive()
            if event.get("event") != "closed" or event.get("request_id") != "close":
                raise ValueError("missing close acknowledgement")
        elif mode == "eof_closes":
            self.child.stdin.close()
            event = self.receive()
            if event.get("event") != "closed" or event.get("reason") != "eof":
                raise ValueError("missing EOF acknowledgement")
        elif mode == "idle_exits":
            if self.receive().get("event") != "idle_timeout":
                raise ValueError("missing idle expiry")
        else:
            if self.child.poll() is not None:
                raise ValueError("session exited before deliberate kill")
            self.child.kill()
        code = self.child.wait(timeout=15)
        if (mode == "forced_kills") != (code != 0):
            raise ValueError(f"unexpected termination status: {mode}/{code}")
        self.reader.join(timeout=3)
        if self.reader.is_alive():
            raise ValueError("stdout reader failed to stop")
        if not self.child.stdin.closed:
            self.child.stdin.close()
        self.child.stdout.close()

    def stop(self):
        self.stopping.set()
        if self.child.poll() is None:
            self.child.kill()
            self.child.wait(timeout=15)
        if not self.child.stdin.closed:
            self.child.stdin.close()
        self.reader.join(timeout=3)
        self.child.stdout.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--crate", required=True)
    parser.add_argument("--fixture", required=True, help="immutable store plus queries.fvecs")
    parser.add_argument("--output", required=True, help="new directory; no continuation")
    parser.add_argument("--seconds", type=int, default=86400)
    parser.add_argument("--requests", type=int, default=100000)
    parser.add_argument("--batch-size", type=int, default=4096)
    parser.add_argument("--reference-queries", type=int, default=100)
    parser.add_argument("--budget", type=int, default=64)
    parser.add_argument("--idle-test-ms", type=int, default=5000)
    parser.add_argument("--toolchain", default="1.89.0")
    args = parser.parse_args()
    if (not 0 <= args.seconds <= 604800 or args.requests < 1
            or not 1 <= args.batch_size <= 4096 or not 1 <= args.reference_queries <= 100
            or not 10 <= args.budget <= 65536 or not 100 <= args.idle_test_ms <= 60000):
        parser.error("invalid bounded duration, request, query, candidate or idle parameters")
    archive = Path(args.crate).resolve()
    fixture = Path(args.fixture).resolve()
    root = Path(args.output).resolve()
    check(archive)
    root.mkdir(parents=True, exist_ok=False)
    case = root / "case"
    case.mkdir()
    with tarfile.open(archive) as package:
        package.extractall(root / "source", filter="data")
    source = next((root / "source").iterdir())
    if digest(Path(__file__)) != digest(source / "tools/run_session_soak.py"):
        raise ValueError("runner must match the exact archived QA source")
    record = dict(schema_version=1, completed=False, phase="build", crate_sha256=digest(archive),
                  source_sha256=fingerprint(source), minimum_seconds=args.seconds,
                  minimum_requests=args.requests, toolchain=args.toolchain,
                  proof_scope="exact-archive source QA; registry identity requires separate verification",
                  fixture_before=fixture_inventory(fixture), preparation_started_utc=utc())
    receipt = root / "receipt.json"
    write_json(receipt, record)
    progress = dict(completed=False, elapsed_seconds=0, verified_requests=0, reference_queries=0,
                    session_starts=0, session_sq8_builds=0, normal_closes=0, eof_closes=0,
                    forced_kills=0, idle_exits=0, invalid_requests_recovered=0,
                    active_lease_refusals=0, released_cache_leases=0, cache_preparations=0,
                    max_sampled_engine_rss_bytes=None)
    active = None
    start = None

    def command(*parts, check_result=True):
        result = subprocess.run([str(binary), *map(str, parts)], capture_output=True,
                                text=True, encoding="utf-8", timeout=90)
        if check_result and result.returncode:
            raise RuntimeError(result.stderr.strip())
        return result

    def save_progress():
        progress["elapsed_seconds"] = 0 if start is None else time.monotonic() - start
        progress["duration_gate_24h"] = progress["elapsed_seconds"] >= 86400
        progress["requests_gate_100k"] = progress["verified_requests"] >= 100000
        progress["full_source_gate"] = duration_gates(progress)
        progress["active_pid"] = None if active is None else active.child.pid
        write_json(case / "progress.json", progress)

    def interrupted(_number, _frame):
        raise KeyboardInterrupt("QA interrupted; this run cannot be resumed as full-duration evidence")
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    try:
        subprocess.run(["cargo", "+" + args.toolchain, "build", "--release", "--offline", "--locked",
                        "--manifest-path", str(source / "Cargo.toml"), "--bin", "vecnook",
                        "--target-dir", str(root / "build")], check=True, timeout=300)
        binary = root / "build/release" / ("vecnook.exe" if os.name == "nt" else "vecnook")
        record.update(binary_sha256=digest(binary), phase="independent resident references")
        write_json(receipt, record)
        queries = vectors(fixture / "queries.fvecs")[:args.reference_queries]
        references = []
        for index, vector in enumerate(queries):
            path = case / f"query-{index}.txt"
            path.write_text(",".join(map(repr, vector)), encoding="utf-8")
            raw = decode(command("snapshot-search", fixture / "store", "@" + str(path), "10",
                                 "--sq8", str(args.budget)).stdout)
            if raw.get("schema_version") != 1:
                raise ValueError("reference schema changed")
            references.append(canonical(raw))
        write_json(case / "resident-references.json", references)
        progress["reference_queries"] = len(queries)
        cache = case / "prepared-cache"
        modes = ["normal_closes", "eof_closes", "forced_kills", "idle_exits"]
        record.update(phase="running", memory_metric="sampled ps RSS bytes, not native high-water; unavailable on Windows")
        last_progress = time.monotonic()
        with (root / "sessions.stderr.log").open("w", encoding="utf-8") as stderr:
            while True:
                command("snapshot-prepare-sq8", fixture / "store", cache)
                progress["cache_preparations"] += 1
                mode = modes[progress["session_starts"] % 4]
                idle = args.idle_test_ms if mode == "idle_exits" else 60000
                active = Session(binary, fixture / "store", cache, args.budget, idle, stderr)
                if start is None:
                    start = time.monotonic()
                    record["started_utc"] = utc()
                    write_json(receipt, record)
                progress["session_starts"] += 1
                progress["session_sq8_builds"] += active.ready["sq8_builds"]
                refusal = command("--json-errors", "snapshot-prune-sq8", cache, check_result=False)
                if refusal.returncode == 0 or decode(refusal.stderr)["error"]["kind"] != "Locked":
                    raise ValueError("live session cache lease did not protect DATA")
                progress["active_lease_refusals"] += 1
                active.invalid_then_recover()
                progress["invalid_requests_recovered"] += 2
                save_progress()
                for _ in range(args.batch_size):
                    number = progress["verified_requests"]
                    index = number % len(queries)
                    active.request(number, queries[index], references[index])
                    progress["verified_requests"] += 1
                    if time.monotonic() - last_progress >= 10:
                        if os.name != "nt":
                            observed = subprocess.run(["ps", "-o", "rss=", "-p", str(active.child.pid)],
                                                      capture_output=True, text=True, timeout=5)
                            if observed.returncode == 0 and observed.stdout.strip().isdigit():
                                size = int(observed.stdout) * 1024
                                progress["max_sampled_engine_rss_bytes"] = max(progress["max_sampled_engine_rss_bytes"] or 0, size)
                        save_progress()
                        last_progress = time.monotonic()
                    if (progress["verified_requests"] >= args.requests and progress["session_starts"] >= 4
                            and time.monotonic() - start >= args.seconds):
                        break
                    if progress["verified_requests"] >= args.requests:
                        time.sleep(0.05)
                active.finish(mode)
                active = None
                progress[mode] += 1
                command("snapshot-prune-sq8", cache)
                progress["released_cache_leases"] += 1
                save_progress()
                if (progress["verified_requests"] >= args.requests and progress["session_starts"] >= 4
                        and time.monotonic() - start >= args.seconds):
                    break
        record["fixture_after"] = fixture_inventory(fixture)
        if record["fixture_after"] != record["fixture_before"] or fingerprint(source) != record["source_sha256"]:
            raise ValueError("frozen fixture or archived source changed")
        progress["completed"] = True
        save_progress()
        record.update(completed=True, exit_code=0, phase="finished", finished_utc=utc(), progress=progress)
        write_json(receipt, record)
        print(json.dumps(dict(receipt=str(receipt), completed=True, full_source_gate=duration_gates(progress))))
    except BaseException as error:
        if active is not None:
            active.stop()
            active = None
        save_progress()
        record.update(completed=False, exit_code=1, phase="failed", finished_utc=utc(),
                      error=str(error), progress=progress)
        write_json(receipt, record)
        raise


if __name__ == "__main__":
    main()
