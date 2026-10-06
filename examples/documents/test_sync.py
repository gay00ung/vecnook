"""Incremental sync contracts with offline model transport and real CLI storage."""
import contextlib
import io
import json
from pathlib import Path
import unittest
from unittest.mock import patch
import search
import test_search as fixture
Handler = fixture.Handler


class SyncTests(unittest.TestCase):
    setUp = fixture.IntegrationTests.setUp
    tearDown = fixture.IntegrationTests.tearDown

    def run_sync(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            search.sync_documents(self.args, self.client)
        return output.getvalue()

    def state(self):
        manifest = json.loads(search.cli(self.args.binary, "docs-info", self.args.db.parent, self.args.db.name))
        return search.collection_state(self.args, manifest)

    def test_unchanged_files_skip_embeddings_and_edits_keep_existing_ids(self):
        self.run_sync()
        before, sequence, _ = self.state()
        requests = len(self.server.inputs)
        self.assertIn("unchanged=1", self.run_sync())
        self.assertEqual(len(self.server.inputs), requests)
        self.assertEqual(self.state()[1], sequence)
        source = self.documents / '한글 notes 🚀.md'
        source.write_bytes(b"updated original\n")
        self.assertIn("changed=1", self.run_sync())
        after, _, _ = self.state()
        self.assertEqual(after[0]["id"], before[0]["id"])
        self.assertEqual(after[0]["text"], "updated original\n")
        self.assertEqual(len(self.server.inputs), requests + 1)
        mixed = "한글\r\nsecond\n"
        source.write_bytes(mixed.encode("utf-8"))
        self.run_sync()
        self.assertEqual(self.state()[0][0]["text"], mixed)

    def test_chunk_growth_shrink_empty_and_restart_derive_state_from_database(self):
        source = self.documents / '한글 notes 🚀.md'
        source.write_text("a" * 8000, encoding="utf-8")
        self.run_sync()
        first = self.state()[0]
        source.write_text("b" * 11000, encoding="utf-8")
        self.run_sync()
        grown = self.state()[0]
        self.assertEqual([d["id"] for d in grown[:len(first)]], [d["id"] for d in first])
        self.assertEqual("".join(d["text"] for d in grown), "b" * 11000)
        requests = len(self.server.inputs)
        self.assertIn("unchanged=1", self.run_sync())
        self.assertEqual(len(self.server.inputs), requests)
        source.write_text("short", encoding="utf-8")
        self.run_sync()
        self.assertEqual([d["text"] for d in self.state()[0]], ["short"])
        source.write_text("", encoding="utf-8")
        self.run_sync()
        self.assertEqual(self.state()[0], [])

    def test_prune_requires_opt_in_and_preserves_other_namespaces(self):
        self.run_sync()
        _, sequence, space = self.state()
        foreign = Path(self.root / "foreign.tsv")
        payload = search.encode_document(100, search.Chunk("outside.md", 1, 1, "foreign"), ["manual"])
        foreign.write_text(f"put\t100\t1,0\t{payload}\n", encoding="utf-8")
        search.cli(self.args.binary, "docs-batch", *space, foreign, "--if-sequence", sequence)
        for source in self.documents.iterdir():
            source.unlink()
        self.run_sync()
        self.assertEqual(len(self.state()[0]), 2)
        self.args.prune = True
        self.assertIn("deleted=1", self.run_sync())
        self.assertEqual([d["id"] for d in self.state()[0]], ["100"])

    def test_failed_scan_model_and_embedding_never_replace_old_source_or_prune(self):
        self.run_sync()
        before = self.state()[:2]
        self.args.prune = True
        with patch("search.os.scandir", side_effect=PermissionError("denied")):
            with self.assertRaises(PermissionError):
                self.run_sync()
        self.assertEqual(self.state()[:2], before)
        (self.documents / '한글 notes 🚀.md').write_text("changed", encoding="utf-8")
        Handler.broken = True
        with self.assertRaisesRegex(ValueError, "count mismatch"):
            self.run_sync()
        self.assertEqual(self.state()[:2], before)
        Handler.broken, Handler.digest = False, "different"
        requests = len(self.server.inputs)
        with self.assertRaisesRegex(ValueError, "model changed"):
            self.run_sync()
        self.assertEqual(len(self.server.inputs), requests)
        self.assertEqual(self.state()[:2], before)

    def test_oversized_source_and_stale_sequence_fail_before_writes(self):
        self.run_sync()
        before, sequence, space = self.state()
        (self.documents / '한글 notes 🚀.md').write_text("x" * 4100, encoding="utf-8")
        requests = len(self.server.inputs)
        with patch("search.CHUNK_BYTES", 4):
            with self.assertRaisesRegex(ValueError, "1024-operation"):
                self.run_sync()
        self.assertEqual(self.state()[:2], (before, sequence))
        self.assertEqual(len(self.server.inputs), requests)
        batch = self.root / "stale.tsv"
        batch.write_text(f"delete\t{before[0]['id']}\n", encoding="utf-8")
        search.cli(self.args.binary, "docs-batch", *space, batch, "--if-sequence", sequence)
        after = self.state()[:2]
        with self.assertRaisesRegex(ValueError, "conflict"):
            search.cli(self.args.binary, "docs-batch", *space, batch, "--if-sequence", sequence)
        self.assertEqual(self.state()[:2], after)

    def test_rename_prunes_old_source_and_unmanaged_collisions_are_protected(self):
        self.run_sync()
        (self.documents / '한글 notes 🚀.md').rename(self.documents / "renamed.md")
        self.args.prune = True
        self.run_sync()
        self.assertEqual([d["source"] for d in self.state()[0]], ["renamed.md"])
        _, sequence, space = self.state()
        payload = search.encode_document(100, search.Chunk("manual.md", 1, 1, "original"), [])
        batch = self.root / "manual.tsv"
        batch.write_text(f"put\t100\t1,0\t{payload}\n", encoding="utf-8")
        search.cli(self.args.binary, "docs-batch", *space, batch, "--if-sequence", sequence)
        (self.documents / "manual.md").write_text("do not overwrite", encoding="utf-8")
        before = self.state()[:2]
        with self.assertRaisesRegex(ValueError, "unmanaged"):
            self.run_sync()
        self.assertEqual(self.state()[:2], before)


if __name__ == "__main__":
    unittest.main()
