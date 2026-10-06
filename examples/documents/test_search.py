"""Offline transport and persistence tests; fixture vectors are not a quality benchmark."""

import contextlib
import io
import json
import os
from pathlib import Path
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from types import SimpleNamespace
import tempfile
import threading
import unittest

import search


class Handler(BaseHTTPRequestHandler):
    digest = "fixture-model-v1"
    broken = False

    def log_message(self, *_):
        pass

    def do_GET(self):
        self.respond({"models": [{"name": search.DEFAULT_MODEL, "digest": self.digest}]})

    def do_POST(self):
        payload = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        self.server.inputs.append(payload)
        self.respond({"embeddings": [[1.0, 0.0] for _ in payload["input"]][:-1]
                      if self.broken else [[1.0, 0.0] for _ in payload["input"]]})

    def respond(self, payload):
        raw = json.dumps(payload).encode()
        self.send_response(200)
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)


class IntegrationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.documents = self.root / "documents"
        self.documents.mkdir()
        (self.documents / '한글 notes 🚀.md').write_text('# 제목\nline "one"\\\t\nsecond line\n', encoding="utf-8")
        Handler.digest, Handler.broken = "fixture-model-v1", False
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.inputs = []
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.client = search.Ollama(f"http://127.0.0.1:{self.server.server_port}")
        self.args = SimpleNamespace(db=self.root / "db", documents=self.documents,
                                    model=search.DEFAULT_MODEL,
                                    binary=Path(os.environ.get("VECNOOK_BINARY", "target/debug/vecnook")).resolve(),
                                    query="recover my documents", k=3, json=True)

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()
        self.temporary.cleanup()

    def test_original_unicode_source_and_text_survive_cli_restart_and_json(self):
        with contextlib.redirect_stdout(io.StringIO()):
            search.index_documents(self.args, self.client)
        first = io.StringIO()
        with contextlib.redirect_stdout(first):
            search.search_documents(self.args, self.client)
        result = json.loads(first.getvalue())
        record = result["neighbors"][0]
        self.assertEqual(record["source"], '한글 notes 🚀.md')
        self.assertEqual(record["text"], (self.documents / record["source"]).read_text(encoding="utf-8"))
        self.assertEqual((record["start_line"], record["end_line"]), (1, 3))
        self.assertEqual(record["distance"], 0.0)
        self.assertTrue(result["complete"])
        self.assertTrue(all(not request["truncate"] for request in self.server.inputs))
        self.assertTrue(self.server.inputs[0]["input"][0].startswith("title:"))
        self.assertTrue(self.server.inputs[-1]["input"][0].startswith("task: search result | query:"))
        self.assertIn("graph_cache_loaded=true", search.cli(self.args.binary, "stats", self.args.db))

    def test_existing_database_is_never_overwritten(self):
        self.args.db.mkdir()
        sentinel = self.args.db / "keep"
        sentinel.write_text("existing")
        with self.assertRaisesRegex(ValueError, "new database"):
            search.index_documents(self.args, self.client)
        self.assertEqual(sentinel.read_text(), "existing")
        self.assertEqual(self.server.inputs, [])

    def test_bad_embedding_response_creates_no_database(self):
        Handler.broken = True
        with self.assertRaisesRegex(ValueError, "count mismatch"):
            search.index_documents(self.args, self.client)
        self.assertFalse(self.args.db.exists())

    def test_changed_model_is_rejected_before_query_embedding(self):
        with contextlib.redirect_stdout(io.StringIO()):
            search.index_documents(self.args, self.client)
        before = len(self.server.inputs)
        Handler.digest = "different-model-same-dimensions"
        with self.assertRaisesRegex(ValueError, "model changed"):
            search.search_documents(self.args, self.client)
        self.assertEqual(len(self.server.inputs), before)

    def test_indexed_tags_and_source_can_restrict_results_after_restart(self):
        self.args.tags = ["rust", "notes"]
        with contextlib.redirect_stdout(io.StringIO()):
            search.index_documents(self.args, self.client)
        self.args.tags = ["rust", "missing"]
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            search.search_documents(self.args, self.client)
        self.assertEqual(json.loads(output.getvalue())["neighbors"], [])
        self.args.tags = ["rust", "notes"]
        self.args.source = '한글 notes 🚀.md'
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            search.search_documents(self.args, self.client)
        data = json.loads(output.getvalue())
        self.assertEqual(data["eligible_count"], 1)
        self.assertEqual(data["filter_evaluations"], 0)
        self.assertEqual(data["neighbors"][0]["tags"], ["rust", "notes"])

    def test_long_unicode_lines_keep_valid_utf8_and_source_line_numbers(self):
        original = "한글🚀" * 1300
        for file in self.documents.iterdir():
            file.unlink()
        (self.documents / "long.md").write_text(original, encoding="utf-8")
        chunks = search.read_chunks(self.documents)
        self.assertEqual("".join(c.text for c in chunks), original)
        self.assertTrue(all(len(c.text.encode()) <= search.CHUNK_BYTES for c in chunks))
        self.assertTrue(all((c.start_line, c.end_line) == (1, 1) for c in chunks))

    def test_empty_documents_symlinks_and_oversize_files_are_rejected(self):
        for file in self.documents.iterdir():
            file.unlink()
        with self.assertRaisesRegex(ValueError, "nonempty"):
            search.read_chunks(self.documents)
        outside = self.root / "private.md"
        outside.write_text("outside")
        link = self.documents / "link.md"
        try:
            link.symlink_to(outside)
        except OSError:
            pass
        else:
            with self.assertRaisesRegex(ValueError, "symlink"):
                search.read_chunks(self.documents)
            link.unlink()
        (self.documents / "large.md").write_bytes(b"a" * (search.MAX_FILE + 1))
        with self.assertRaisesRegex(ValueError, "limit"):
            search.read_chunks(self.documents)

    def test_remote_embedding_origin_is_rejected(self):
        for origin in ["https://example.com", "http://127.0.0.1/api", "http://user:pw@localhost:11434"]:
            with self.assertRaises(ValueError):
                search.Ollama(origin)


if __name__ == "__main__":
    unittest.main()
