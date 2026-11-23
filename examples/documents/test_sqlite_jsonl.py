"""Local SQLite/JSONL read-only and exact-record integration contracts."""
import json
import os
from pathlib import Path
import sqlite3
import struct
import sys
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import sqlite_jsonl as adapter

QUERY = """SELECT d.id AS id, d.source AS source, d.start_line AS start_line,
 d.end_line AS end_line, d.text AS text, e.vector_json AS vector_json,
 d.tags_json AS tags_json, d.numbers_json AS numbers_json, d.dates_json AS dates_json
 FROM documents AS d JOIN embeddings AS e ON d.id=e.document_id ORDER BY d.id"""


@unittest.skipUnless(hasattr(sqlite3.Connection, "setlimit"), "optional SQLite example needs Python 3.11+")
class SQLiteJsonlTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.db = self.root / "한글 #% input.sqlite"
        self.con = sqlite3.connect(self.db)
        self.con.execute("PRAGMA journal_mode=WAL")
        self.con.execute("CREATE TABLE documents(id TEXT PRIMARY KEY, source TEXT, start_line INTEGER, end_line INTEGER, text TEXT, tags_json TEXT, numbers_json TEXT, dates_json TEXT)")
        self.con.execute("CREATE TABLE embeddings(document_id TEXT PRIMARY KEY, vector_json TEXT)")
        maximum = float.fromhex("0x1.fffffep+127")
        for identifier in ["0", str(2**64-1)]:
            self.con.execute("INSERT INTO documents VALUES(?,?,?,?,?,?,?,?)",
                             (identifier, "notes/한글.md", 2, 3, "원문 🚀\r\nsecond\tline\n",
                              '["fixture"]', '{"rating":4.5}', '{"day":"2026-11-21"}'))
            self.con.execute("INSERT INTO embeddings VALUES(?,?)",
                             (identifier, '['+repr(maximum)+','+repr(-maximum)+',-0]'))
        self.con.commit()
        self.output = self.root / "output.jsonl"
        self.options = dict(model_identity="caller-owned-fixture-v1", dimensions=3)

    def tearDown(self):
        self.con.close()
        self.temporary.cleanup()

    def snapshot(self):
        from hashlib import sha256
        return {p.name:sha256(p.read_bytes()).hexdigest()
                for p in [self.db, Path(str(self.db)+"-wal")] if p.is_file()}

    def export(self, query=QUERY, **options):
        return adapter.export_sqlite(self.db, query, self.output, **(self.options | options))

    def rejects(self, query=QUERY, **options):
        before = self.snapshot()
        with self.assertRaises((ValueError, sqlite3.Error, OSError)):
            self.export(query, **options)
        self.assertFalse(self.output.exists())
        self.assertEqual(self.snapshot(), before)

    def test_join_preserves_u64_text_typed_attributes_and_f32_with_installed_cli(self):
        before = self.snapshot()
        self.assertEqual(self.export(), 2)
        self.assertEqual(self.snapshot(), before)
        header, *lines = self.output.read_text(encoding="utf-8").splitlines()
        self.assertEqual(json.loads(header)["space"], {"dimensions":3,"model":"caller-owned-fixture-v1","metric":"squared_l2"})
        rows = [json.loads(line) for line in lines]
        maximum = float.fromhex("0x1.fffffep+127")
        expected = struct.pack("<fff", maximum, -maximum, -0.0)
        for row in rows:
            self.assertEqual(row["text"], "원문 🚀\r\nsecond\tline\n")
            self.assertEqual(row["numbers"], {"rating":4.5})
            self.assertEqual(row["dates"], {"day":"2026-11-21"})
            self.assertEqual(struct.pack("<fff", *row["vector"]), expected)
        binary = os.environ.get("VECNOOK_BINARY")
        if binary:
            binary = str(Path(binary).resolve())
            restored = self.root / "restored"
            for args in [("docs-import-jsonl", self.output, "--db", restored),
                         ("docs-export-jsonl", "--db", restored, "--output", self.root / "again.jsonl")]:
                result = subprocess.run([binary,*map(str,args)],cwd=self.root,capture_output=True,text=True,timeout=30)
                self.assertEqual(result.returncode,0,result.stderr)
            again = [json.loads(line) for line in (self.root/"again.jsonl").read_text(encoding="utf-8").splitlines()[1:]]
            self.assertEqual({r["id"] for r in again}, {"0",str(2**64-1)})
            for row in again:
                self.assertEqual(struct.pack("<fff", *row["vector"]), expected)
                self.assertEqual(row["text"], rows[0]["text"])

    def test_write_attach_pragma_extensions_and_multiple_statements_are_refused(self):
        for query in ["DELETE FROM documents", "CREATE TABLE forbidden(x)",
                      "ATTACH DATABASE '"+str(self.root/"forbidden.sqlite").replace("'", "''")+"' AS other", "PRAGMA query_only=OFF",
                      "SELECT load_extension('forbidden')", QUERY+"; DELETE FROM documents"]:
            with self.subTest(query=query):
                self.rejects(query)
        self.assertFalse((self.root/"forbidden.sqlite").exists())

    def test_schema_duplicate_ids_invalid_rows_and_existing_output_preserve_inputs(self):
        self.rejects("SELECT * FROM documents")
        self.rejects(QUERY.replace("d.source AS source", "d.source AS id"))
        self.rejects(QUERY.replace("d.id AS id", "'0' AS id"))
        self.con.execute("UPDATE embeddings SET vector_json='[1,2]' WHERE document_id='0'")
        self.con.commit();self.rejects()
        self.con.execute("UPDATE embeddings SET vector_json='[1,2,3]'");self.con.commit()
        self.output.write_text("keep existing output",encoding="utf-8")
        before=self.snapshot()
        with self.assertRaises(FileExistsError):self.export()
        self.assertEqual(self.output.read_text(),"keep existing output")
        self.assertEqual(self.snapshot(),before)

    def test_row_bytes_document_fields_and_query_bounds(self):
        self.rejects("x"*(adapter.MAX_SQL_BYTES+1))
        for expression in ["'01'", "'18446744073709551616'", "NULL"]:
            with self.subTest(id=expression):self.rejects(QUERY.replace("d.id AS id",expression+" AS id"))
        for expression in ["'{}'", "'[true,2,3]'", "'[1e999,2,3]'", "'[0,0,0]'"]:
            with self.subTest(vector=expression):self.rejects(QUERY.replace("e.vector_json AS vector_json",expression+" AS vector_json"),metric="cosine")
        self.rejects(QUERY.replace("d.text AS text", "printf('%*s', 6001, 'x') AS text"))
        self.rejects(QUERY.replace("d.text AS text", "printf('%*s', 131073, 'x') AS text"))
        self.rejects(QUERY, model_identity="")
        self.rejects(QUERY, dimensions=True)
        with patch.object(adapter,"MAX_ROWS",1):self.rejects()
        with patch.object(adapter,"MAX_OUTPUT_BYTES",128):self.rejects()

    def test_query_budget_missing_database_and_empty_results(self):
        infinite="WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n) SELECT sum(x) AS id, '' AS source, 1 AS start_line, 1 AS end_line, '' AS text, '[]' AS vector_json, '[]' AS tags_json, '{}' AS numbers_json, '{}' AS dates_json FROM n"
        self.rejects(infinite)
        missing=self.root/"missing.sqlite"
        with self.assertRaises(FileNotFoundError):adapter.export_sqlite(missing,QUERY,self.output,**self.options)
        self.assertFalse(missing.exists());self.assertFalse(self.output.exists())
        self.assertEqual(self.export(QUERY.replace("ORDER BY d.id", "WHERE 0")),0)
        self.assertEqual(len(self.output.read_text().splitlines()),1)

    def test_typed_fields_lines_and_runtime_guard_fail_before_output(self):
        for old, new in [("d.source AS source", "'' AS source"),
                         ("d.start_line AS start_line", "0 AS start_line"),
                         ("d.start_line AS start_line", "1.5 AS start_line"),
                         ("d.tags_json AS tags_json", "'[1]' AS tags_json"),
                         ("d.numbers_json AS numbers_json", "'{\"x\":true}' AS numbers_json"),
                         ("d.dates_json AS dates_json", "'{\"day\":\"2026-02-29\"}' AS dates_json")]:
            with self.subTest(field=new):self.rejects(QUERY.replace(old,new))
        self.rejects(QUERY,metric=[])
        with patch.object(adapter.sqlite3,"Connection",type("WithoutSetLimit",(),{})):
            self.rejects()

    def test_cli_roundtrip_and_validation_failures_preserve_existing_paths(self):
        sql=self.root/"query.sql";sql.write_text(QUERY,encoding="utf-8")
        before=self.snapshot()
        args=[sys.executable,"-B",str(Path(adapter.__file__).resolve()),str(self.db),
              "--sql",str(sql),"--output",str(self.output),"--dimensions","3",
              "--model-identity",self.options["model_identity"]]
        success=subprocess.run(args,cwd=self.root,capture_output=True,text=True,timeout=10)
        self.assertEqual(success.returncode,0,success.stderr)
        original=self.output.read_bytes()
        existing=subprocess.run(args,cwd=self.root,capture_output=True,text=True,timeout=10)
        self.assertEqual(existing.returncode,2)
        self.assertEqual(self.output.read_bytes(),original)
        invalid=self.root/"invalid.jsonl"
        args[args.index("--output")+1]=str(invalid)
        sql.write_bytes(b"\xff")
        rejected=subprocess.run(args,cwd=self.root,capture_output=True,text=True,timeout=10)
        self.assertEqual(rejected.returncode,2)
        self.assertIn("sqlite-jsonl:",rejected.stderr)
        self.assertFalse(invalid.exists())
        self.assertEqual(self.snapshot(),before)

    def test_wal_reader_holds_one_snapshot_during_committed_updates(self):
        reader=adapter._rows(self.db,QUERY)
        try:
            first=next(reader)
            self.con.execute("UPDATE documents SET text='changed after snapshot'")
            self.con.commit()
            remaining=list(reader)
        finally:reader.close()
        self.assertEqual(len(remaining),1)
        self.assertEqual(first["text"],remaining[0]["text"])
        self.assertNotEqual(self.con.execute("SELECT text FROM documents LIMIT 1").fetchone()[0],first["text"])


if __name__ == "__main__":
    unittest.main()
