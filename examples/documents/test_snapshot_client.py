"""Retained Python readers: real subprocess deadlines and native generation leases."""
import gc
from concurrent.futures import ThreadPoolExecutor
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch
import weakref

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "clients" / "python"))
from vecnook import SnapshotSession, VecnookError


FAKE_READER = r'''
import json, sys, time
sys.stdin.reconfigure(encoding="utf-8")
sys.stdout.reconfigure(encoding="utf-8")
mode = sys.argv[3]
def emit(value):
    print(json.dumps(value, ensure_ascii=False), flush=True)
if mode == "startup-error":
    print(json.dumps({"error":{"kind":"Locked","message":"reader is locked"}}), file=sys.stderr, flush=True)
    sys.exit(1)
emit({"schema_version":1,"event":"ready","generation":"7","sequence":"9",
      "snapshot_opens":1,"sq8_builds":0,"cache_hits":0})
if mode == "blocked-write":
    time.sleep(30)
for line in sys.stdin:
    query = json.loads(line)
    ident = query["request_id"]
    if query.get("op") == "close":
        emit({"schema_version":1,"event":"closed","request_id":ident})
        break
    if mode == "hang":
        time.sleep(30)
    if mode == "eof":
        break
    if mode == "malformed":
        print("not JSON", flush=True)
        continue
    if mode == "oversized":
        print("x" * 1024, flush=True)
        continue
    if mode == "unterminated":
        sys.stdout.write('{"schema_version":1}')
        sys.stdout.flush()
        break
    if mode == "utf16":
        sys.stdout.buffer.write('{"schema_version":1}\n'.encode("utf-16-le"))
        sys.stdout.buffer.flush()
        break
    if mode == "duplicate":
        print('{"schema_version":1,"schema_version":1}', flush=True)
        continue
    if mode == "nonfinite":
        print('{"schema_version":1,"search_ms":NaN}', flush=True)
        continue
    if mode == "overflow":
        print('{"schema_version":1,"search_ms":1e999}', flush=True)
        continue
    if mode == "native-error" and query["vector"] == [0]:
        emit({"schema_version":1,"request_id":ident,
              "error":{"kind":"DimensionMismatch","message":"wrong dimension"}})
        continue
    emit({"schema_version":True if mode == "schema" else 1,
          "request_id":"wrong" if mode == "request-id" else ident,
          "snapshot_opens":1,"sq8_builds":0,
          "result":{"generation":"8" if mode == "generation" else "7",
                    "sequence":"9","neighbors":[{"id":query.get("ids", ["1"])[0],
                    "metadata":"원문\n두 번째 🚀"}],"query":query}})
'''


class SessionTransportTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="vecnook-python-session-")
        self.addCleanup(self.temporary.cleanup)
        script = Path(self.temporary.name) / "reader.py"
        script.write_text(FAKE_READER, encoding="utf-8")
        original = subprocess.Popen
        self.children = []

        def spawn(command, **kwargs):
            child = original([sys.executable, str(script), *command[1:]], **kwargs)
            self.children.append(child)
            return child

        self.patcher = patch("vecnook.subprocess.Popen", side_effect=spawn)
        self.patcher.start()
        self.addCleanup(self.patcher.stop)
        self.addCleanup(self.reap)

    def reap(self):
        for child in self.children:
            if child.poll() is None:
                child.kill()
                child.wait(timeout=5)
            for stream in (child.stdin, child.stdout, child.stderr):
                if not stream.closed:
                    stream.close()

    def test_reuses_one_process_preserves_values_and_streams_lazily(self):
        consumed = []

        def vectors():
            for index in range(3):
                consumed.append(index)
                yield [1.0, -0.0]

        with SnapshotSession("good", timeout=5) as reader:
            info = reader.info()
            info["generation"] = "changed by caller"
            self.assertEqual(reader.info()["generation"], "7")
            batch = reader.search_batch(vectors(), 1, ids=iter(["18446744073709551615"]))
            self.assertEqual(consumed, [])
            first = next(batch)
            self.assertEqual(consumed, [0])
            self.assertEqual(first["request_id"], "1")
            self.assertEqual(first["result"]["neighbors"], [
                {"id": "18446744073709551615", "metadata": "원문\n두 번째 🚀"}])
            self.assertEqual(struct.pack("<f", first["result"]["query"]["vector"][1]), b"\0\0\0\x80")
            self.assertEqual([r["request_id"] for r in batch], ["2", "3"])
            self.assertEqual(len(self.children), 1)
        self.assertTrue(reader.closed)
        self.assertEqual(self.children[0].returncode, 0)
        reader.close()

    def test_native_query_error_keeps_reader_usable_and_batch_is_partial(self):
        with SnapshotSession("native-error", timeout=5) as reader:
            batch = reader.search_batch([[1], [0], [2]], 1)
            self.assertEqual(next(batch)["request_id"], "1")
            with self.assertRaises(VecnookError) as error:
                next(batch)
            self.assertEqual(error.exception.kind, "DimensionMismatch")
            self.assertFalse(reader.closed)
            self.assertEqual(reader.search_vector([2], 1)["request_id"], "3")

    def test_startup_error_preserves_native_category_and_reaps(self):
        with self.assertRaises(VecnookError) as error:
            SnapshotSession("startup-error", timeout=5)
        self.assertEqual(error.exception.kind, "Locked")
        self.assertEqual(str(error.exception), "reader is locked")
        self.assertIsNotNone(self.children[0].poll())

    def test_protocol_failures_close_and_reap_without_reopening(self):
        for mode in ["malformed", "oversized", "unterminated", "utf16", "duplicate",
                     "nonfinite", "overflow", "schema", "request-id", "generation", "eof"]:
            with self.subTest(mode=mode):
                reader = SnapshotSession(mode, timeout=5, max_response_bytes=512)
                with self.assertRaises(VecnookError) as error:
                    reader.search_vector([1], 1)
                self.assertEqual(error.exception.kind, "Transport")
                self.assertTrue(reader.closed)
                self.assertIsNotNone(self.children[-1].poll())
                reader.close()
        self.assertEqual(len(self.children), 11)

    def test_callers_share_one_reader_without_crossed_responses(self):
        with SnapshotSession("good", timeout=5) as reader:
            def search(index):
                reply = reader.search_vector([index], 1)
                return reply["request_id"], reply["result"]["query"]["vector"]

            with ThreadPoolExecutor(max_workers=4) as executor:
                results = list(executor.map(search, range(24)))
            self.assertEqual([query for _, query in results], [[index] for index in range(24)])
            self.assertEqual({int(ident) for ident, _ in results}, set(range(1, 25)))
            self.assertEqual(len(self.children), 1)

    def test_read_and_blocked_pipe_write_share_bounded_deadline(self):
        for mode in ["hang", "blocked-write"]:
            with self.subTest(mode=mode):
                reader = SnapshotSession(mode, timeout=5)
                reader.timeout = 0.15
                start = time.monotonic()
                with self.assertRaisesRegex(VecnookError, "timed out"):
                    reader.search_vector([1], 1, ids=["18446744073709551615"] * 30000)
                self.assertLess(time.monotonic() - start, 5)
                self.assertTrue(reader.closed)
                self.assertIsNotNone(self.children[-1].poll())

    def test_invalid_inputs_do_not_write_and_reader_recovers(self):
        with SnapshotSession("good", timeout=5) as reader:
            for vector in [[], [True], [float("nan")], [float("inf")], [10**1000], [0] * 4097]:
                with self.subTest(vector_length=len(vector)), self.assertRaises(ValueError):
                    reader.search_vector(vector, 1)
            for ids in ["12", [1], ["01"], ["-1"], ["18446744073709551616"], ["١"], ["1"] * 65537]:
                with self.subTest(ids_length=len(ids)), self.assertRaises(ValueError):
                    reader.search_vector([1], 1, ids=ids)
            self.assertEqual(reader.search_vector([1], 1)["request_id"], "1")

    def test_invalid_constructor_options_do_not_spawn(self):
        for options in [{"timeout": 10**1000}, {"timeout": True}, {"timeout": float("nan")},
                        {"idle_ms": 0}, {"idle_ms": True}, {"sq8": 0}, {"sq8_cache": "cache"},
                        {"buffer_bytes": 32}, {"max_response_bytes": 64 * 1024 * 1024 + 1}]:
            with self.subTest(options=options), self.assertRaises(ValueError):
                SnapshotSession("good", **options)
        self.assertEqual(self.children, [])

    def test_context_exception_and_unreferenced_reader_release_process(self):
        with self.assertRaisesRegex(RuntimeError, "application failure"):
            with SnapshotSession("good", timeout=5):
                raise RuntimeError("application failure")
        self.assertEqual(self.children[-1].returncode, 0)
        reader = SnapshotSession("good", timeout=5)
        reference = weakref.ref(reader)
        child = self.children[-1]
        del reader
        gc.collect()
        self.assertIsNone(reference())
        self.assertIsNotNone(child.poll())


class NativeSessionTests(unittest.TestCase):
    def setUp(self):
        workspace = Path(__file__).resolve().parents[2]
        configured = os.environ.get("VECNOOK_BINARY")
        self.binary = Path(configured) if configured else workspace / "target" / "release" / (
            "vecnook.exe" if os.name == "nt" else "vecnook")
        self.binary = self.binary.resolve()
        if not self.binary.is_file():
            self.skipTest("build the CLI or set VECNOOK_BINARY for native session assertions")
        self.temporary = tempfile.TemporaryDirectory(prefix="vecnook-native-session-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.db, self.store, self.cache = [self.root / name for name in ("raw", "store", "cache")]
        self.metadata = "원문\n두 번째 🚀"
        self.native("init", self.db, 2)
        self.native("put", self.db, "18446744073709551615", "1,-0.0", self.metadata)
        self.native("put", self.db, "1", "0,1", "other")
        self.native("snapshot-publish", self.db, self.store, "python-session")

    def native(self, *args, success=True):
        result = subprocess.run([str(self.binary), "--json-errors", *map(str, args)],
                                cwd=self.root, capture_output=True, text=True,
                                encoding="utf-8", timeout=15)
        self.assertEqual(result.returncode == 0, success, result.stderr)
        return result.stdout if success else result.stderr

    def session(self, **options):
        reader = SnapshotSession(self.store, binary=self.binary, timeout=10, **options)
        self.addCleanup(reader.close)
        return reader

    def test_native_reuse_errors_original_records_and_full_u64(self):
        with self.session(sq8=16) as reader:
            self.assertEqual((reader.info()["snapshot_opens"], reader.info()["sq8_builds"]), (1, 1))
            with self.assertRaises(VecnookError):
                reader.search_vector([1], 1)
            for reply in reader.search_batch(([1, -0.0] for _ in range(256)), 1,
                                             ids=["18446744073709551615"]):
                self.assertEqual((reply["snapshot_opens"], reply["sq8_builds"]), (1, 1))
                self.assertEqual(reply["result"]["neighbors"][0]["metadata"], self.metadata)
                self.assertEqual(reply["result"]["neighbors"][0]["id"], "18446744073709551615")
        record = json.loads(self.native("snapshot-get", self.store, "18446744073709551615"))["record"]
        self.assertEqual(record["metadata"], self.metadata)
        self.assertEqual(struct.pack("<2f", *record["vector"]), struct.pack("<2f", 1, -0.0))

    def test_new_current_does_not_refresh_live_reader_and_gc_respects_pin(self):
        self.native("snapshot-gc", self.store)
        old = self.session()
        generation = old.info()["generation"]
        self.native("put", self.db, "18446744073709551615", "1,-0.0", "new original")
        self.native("snapshot-publish", self.db, self.store, "python-session")
        self.assertEqual(json.loads(self.native("snapshot-gc", self.store))["removed_generations"], 0)
        self.assertEqual(old.search_vector([1, 0], 1)["result"]["neighbors"][0]["metadata"], self.metadata)
        with self.session() as new:
            self.assertNotEqual(new.info()["generation"], generation)
            self.assertEqual(new.search_vector([1, 0], 1)["result"]["neighbors"][0]["metadata"], "new original")
        old.close()
        self.assertEqual(json.loads(self.native("snapshot-gc", self.store))["removed_generations"], 1)

    def test_prepared_cache_is_explicit_pinned_and_released_after_close(self):
        with self.assertRaises(VecnookError):
            self.session(sq8=16, sq8_cache=self.cache)
        self.assertFalse(self.cache.exists())
        self.native("snapshot-prepare-sq8", self.store, self.cache)
        with self.session(sq8=16, sq8_cache=self.cache, buffer_bytes=36) as reader:
            info = reader.info()
            self.assertEqual((info["cache_hits"], info["sq8_builds"], info["code_read_buffer_bytes"]), (1, 0, 36))
            reader.search_vector([1, 0], 1)
            error = json.loads(self.native("snapshot-prune-sq8", self.cache, success=False))
            self.assertEqual(error["error"]["kind"], "Locked")
        self.assertGreater(json.loads(self.native("snapshot-prune-sq8", self.cache))["removed_files"], 0)

    def test_idle_exit_is_observable_and_close_reaps(self):
        reader = self.session(idle_ms=50)
        reader._process.wait(timeout=5)
        self.assertTrue(reader.closed)
        with self.assertRaises(VecnookError):
            reader.search_vector([1, 0], 1)
        reader.close()
        self.assertTrue(reader._process.stdin.closed)


if __name__ == "__main__":
    unittest.main()
