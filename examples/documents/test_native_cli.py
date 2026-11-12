"""Installed-binary folder commands with a local, deterministic embedding server."""
import json
import os
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch
import test_search as fixture


class NativeCliTests(unittest.TestCase):
    setUp = fixture.IntegrationTests.setUp
    tearDown = fixture.IntegrationTests.tearDown

    def native(self, *arguments, success=True):
        # A temporary cwd proves the CLI does not load a script from the checkout.
        binary = Path(self.args.binary).resolve()
        result = subprocess.run([binary, *map(str, arguments)], cwd=self.root,
                                capture_output=True, text=True, encoding="utf-8", timeout=30)
        self.assertEqual(result.returncode == 0, success, result.stderr)
        return result.stdout

    def test_folder_sync_hybrid_keyword_vector_updates_and_prune(self):
        origin = self.client.url
        args = [self.documents, "--db", self.args.db, "--ollama-url", origin]
        self.assertIn("changed=1", self.native("docs-sync", *args))
        before = len(self.server.inputs)
        self.assertIn("unchanged=1", self.native("docs-sync", *args))
        self.assertEqual(len(self.server.inputs), before)
        for mode in ["hybrid", "keyword", "vector"]:
            result = json.loads(self.native("docs-find", "제목", "--db", self.args.db,
                                "--mode", mode, "--json", "--ollama-url", origin))
            neighbor = result["neighbors"][0]
            self.assertEqual(neighbor["source"], '한글 notes 🚀.md')
            self.assertEqual(neighbor["id"], "0")
            self.assertEqual(neighbor["tags"], [])
            self.assertEqual(result["schema_version"], 1)
            if mode == "hybrid":
                self.assertEqual((neighbor["vector_rank"], neighbor["keyword_rank"]), (1, 1))
        source = self.documents / '한글 notes 🚀.md'
        source.write_bytes(b"ERR-42 updated text\r\nsecond line\n")
        self.assertIn("changed=1", self.native("docs-sync", *args))
        result = json.loads(self.native("docs-find", "ERR-42", "--db", self.args.db,
                                       "--mode", "keyword", "--json", "--ollama-url", origin))
        self.assertEqual(result["neighbors"][0]["text"], source.read_bytes().decode("utf-8"))
        source.unlink()
        self.assertIn("deleted=1", self.native("docs-sync", *args, "--prune"))
        result = json.loads(self.native("docs-find", "ERR-42", "--db", self.args.db,
                                       "--mode", "keyword", "--json", "--ollama-url", origin))
        self.assertEqual(result["neighbors"], [])

    def test_prune_only_removes_missing_sources_in_scanned_formats(self):
        args = [self.documents, "--db", self.args.db, "--ollama-url", self.client.url, "--json"]
        text = self.documents / "retained.TXT"
        text.write_bytes("텍스트 원문 🚀\r\n".encode("utf-8"))
        markdown = next(self.documents.glob("*.md"))

        def inspect():
            return json.loads(self.native("docs-inspect", self.args.db.parent, self.args.db.name))

        self.native("docs-sync", *args, "--include-text")
        before = inspect()
        self.assertEqual({row["source"] for row in before["documents"]}, {markdown.name, text.name})
        original = next(row for row in before["documents"] if row["source"] == text.name)
        requests = len(self.server.inputs)
        markdown.unlink()

        plan = json.loads(self.native("docs-sync", *args, "--prune", "--dry-run"))
        self.assertEqual(plan["deleted_sources"], [markdown.name])
        self.assertEqual(inspect(), before)
        result = json.loads(self.native("docs-sync", *args, "--prune"))
        self.assertEqual(result["deleted"], 1)
        self.assertEqual(inspect()["documents"], [original])

        text.unlink()
        result = json.loads(self.native("docs-sync", *args, "--prune", "--include-text"))
        self.assertEqual(result["deleted"], 1)
        self.assertEqual(inspect()["documents"], [])
        self.assertEqual(len(self.server.inputs), requests)

    def test_default_find_selects_identifiers_and_keyword_queries_do_not_request_embeddings(self):
        origin = self.client.url
        self.native("docs-sync", self.documents, "--db", self.args.db, "--ollama-url", origin)
        default = json.loads(self.native("docs-find", "ERR_IO_27", "--db", self.args.db,
                                         "--json", "--ollama-url", origin))
        self.assertEqual(default["mode"], "hybrid")
        self.assertEqual(default["retrieval_mode"], "hybrid")
        prose = json.loads(self.native("docs-find", "How do I recover documents?", "--db", self.args.db,
                                       "--json", "--ollama-url", origin))
        self.assertEqual(prose["retrieval_mode"], "vector")
        before = len(self.server.inputs)
        fixture.Handler.broken = True
        fixture.Handler.digest = "changed-model"
        self.native("docs-find", "제목", "--db", self.args.db,
                    "--mode", "keyword", "--ollama-url", origin)
        self.assertEqual(len(self.server.inputs), before)
        self.native("docs-find", "제목", "--db", self.args.db,
                    "--ollama-url", origin, success=False)

    def test_help_and_missing_python_have_actionable_messages(self):
        self.assertIn("--prune", self.native("docs-sync", "--help"))
        self.assertIn("--mode", self.native("docs-find", "--help"))
        binary = Path(self.args.binary).resolve()
        result = subprocess.run([binary, "docs-sync", self.documents, "--db", self.args.db],
                                env=dict(os.environ, VECNOOK_PYTHON=str(self.root / "absent-python")),
                                capture_output=True, text=True, timeout=30)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Python 3.10+", result.stderr)
        self.assertFalse(self.args.db.exists())

    def test_unicode_output_survives_a_non_utf8_parent_environment(self):
        origin = self.client.url
        with patch.dict(os.environ, {"PYTHONIOENCODING": "ascii"}):
            self.native("docs-sync", self.documents, "--db", self.args.db, "--ollama-url", origin)
            result = json.loads(self.native("docs-find", "제목", "--db", self.args.db,
                                           "--mode", "keyword", "--json", "--ollama-url", origin))
        self.assertEqual(result["neighbors"][0]["source"], '한글 notes 🚀.md')
