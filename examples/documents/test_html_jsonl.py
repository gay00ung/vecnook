"""Static HTML contract and installed CLI import/export checks without a model server."""
import json
from itertools import count
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest

import html_jsonl


class HtmlTextTests(unittest.TestCase):
    def document(self, html="<p>text</p>", **options):
        args = dict(identifier="0", source="note.html", vector=[1.0, -0.0],
                    model_identity="application-owned:model:revision")
        args.update(options)
        return [json.loads(line) for line in html_jsonl.collection_jsonl(html, **args).splitlines()]

    def test_text_nodes_blocks_entities_and_original_whitespace(self):
        cases = [
            ("<p>one</p><p>two</p>", "one\ntwo"),
            ("A<b>B</b>  C", "AB  C"),
            ("<pre>  first\r\n    second\n</pre>", "  first\r\n    second"),
            ("<P>원문 &amp; &#x1F50E;&nbsp;&#62;</P>", "원문 & 🔎\u00a0>"),
            ("a<br/>b<hr>c", "a\nb\nc"),
            ("<table><tr><td>A</td><td>B</td></tr></table>", "A\nB"),
            ("<p>one</p> \n<p>two</p>", "one\n \ntwo"),
            ("<div hidden>included</div><img alt='omitted'>", "included"),
            ("<p>unclosed paragraph", "unclosed paragraph"),
        ]
        for html, expected in cases:
            with self.subTest(html=html):
                self.assertEqual(html_jsonl.extract_text(html), expected)

    def test_excluded_elements_comments_and_nested_templates(self):
        html = ("<!doctype html><head><title>omit</title></head>"
                "<style>body{color:red}</style><script>ignore('<p>script</p>')</script>"
                "<template>outer<template>inner</template>end</template>"
                "<noscript><p>omit</p></noscript><!--comment--><p>kept</p>")
        self.assertEqual(html_jsonl.extract_text(html), "kept")
        self.assertEqual(html_jsonl.extract_text("kept<template>unclosed"), "kept")

    def test_utf8_input_and_text_boundaries(self):
        self.assertEqual(html_jsonl.extract_text("가" * 2000), "가" * 2000)
        with self.assertRaises(ValueError):
            html_jsonl.extract_text("가" * 2001)
        padding = html_jsonl.MAX_INPUT_BYTES - len("<p>x</p><!-- -->")
        html = "<p>x</p><!--" + "a" * padding + " -->"
        self.assertEqual(len(html.encode()), html_jsonl.MAX_INPUT_BYTES)
        self.assertEqual(html_jsonl.extract_text(html), "x")
        for invalid in (html + "a", " ", "<script>only script</script>", None):
            with self.subTest(value=type(invalid).__name__), self.assertRaises(ValueError):
                html_jsonl.extract_text(invalid)

    def test_jsonl_identity_lines_tags_and_coordinates(self):
        maximum = float.fromhex("0x1.fffffep+127")
        vector = [maximum, -maximum, -0.0]
        html = "<h1>제목</h1>\r\n<p>본문 &amp; text</p>\n"
        header, row = self.document(html, identifier=str(2**64 - 1), vector=vector,
                                    tags=iter(["html", "fixture"]))
        self.assertEqual(header["space"], {"dimensions": 3,
                         "model": "application-owned:model:revision", "metric": "squared_l2"})
        self.assertEqual((row["id"], row["start_line"], row["end_line"]), (str(2**64 - 1), 1, 3))
        self.assertEqual(row["tags"], ["html", "fixture"])
        self.assertEqual(row["vector"], vector)
        self.assertEqual(struct.pack("<3f", *row["vector"]), struct.pack("<3f", *vector))
        self.assertEqual(row["text"], "제목\n\r\n본문 & text")

    def test_id_and_metadata_limits_reject_before_serialization(self):
        for identifier in ("01", "-1", "1.0", "١", str(2**64), 0, "9" * 5000):
            with self.subTest(identifier=str(identifier)[:25]), self.assertRaises(ValueError):
                self.document(identifier=identifier)
        self.document(source="s" * 1024, model_identity="m" * 512, tags=["t" * 128])
        for options in ({"source": ""}, {"source": "\0"}, {"source": "s" * 1025},
                        {"model_identity": "m" * 513}, {"model_identity": "\0"},
                        {"tags": ["same", "same"]}, {"tags": [""]}, {"tags": ["\0"]},
                        {"tags": ["t" * 129]}, {"tags": [str(i) for i in range(33)]},
                        {"tags": "string"}, {"tags": None}, {"tags": [True]}):
            with self.subTest(options=str(options)[:60]), self.assertRaises(ValueError):
                self.document(**options)
        with self.assertRaises(ValueError):
            self.document(tags=(str(i) for i in count()))

    def test_vector_and_metric_contract(self):
        for vector in ([], [True], [float("nan")], [float("inf")],
                       [html_jsonl.F32_OVERFLOW], [-html_jsonl.F32_OVERFLOW], [1] * 4097):
            with self.subTest(vector=str(vector)[:40]), self.assertRaises(ValueError):
                self.document(vector=vector)
        self.document(vector=[1] * 4096)
        for metric, expected in html_jsonl.METRICS.items():
            self.assertEqual(self.document(metric=metric)[0]["space"]["metric"], expected)
        for options in ({"metric": "unknown"}, {"metric": []},
                        {"metric": "cosine", "vector": [0.0, -0.0]},
                        {"metric": "cosine", "vector": [2**-150, -1e-200]}):
            with self.assertRaises(ValueError):
                self.document(**options)


class HtmlCliTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="vecnook-html-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.script = Path(html_jsonl.__file__).resolve()
        self.html = self.root / "문서.html"
        self.html.write_bytes("<h1>제목</h1>\r\n<p>본문 &amp; text</p>\n".encode())
        self.vector = self.root / "vector.json"
        maximum = float.fromhex("0x1.fffffep+127")
        self.coordinates = [maximum, -maximum, -0.0]
        self.vector.write_text(json.dumps(self.coordinates), encoding="utf-8")
        self.output = self.root / "document.jsonl"

    def adapter(self, *extra, success=True):
        args = [sys.executable, "-B", str(self.script), str(self.html), "--vector", str(self.vector),
                "--id", str(2**64 - 1), "--model-identity", "application-owned:model:revision",
                "--tag", "html", "--tag", "fixture", "--output", str(self.output), *extra]
        result = subprocess.run(args, cwd=self.root, capture_output=True, text=True,
                                encoding="utf-8", timeout=15)
        self.assertEqual(result.returncode, 0 if success else 2, result.stderr)
        return result

    def test_cli_refuses_overwrite_and_preserves_inputs(self):
        original = self.html.read_bytes(), self.vector.read_bytes()
        self.adapter()
        before = self.output.read_bytes()
        self.adapter(success=False)
        self.assertEqual(self.output.read_bytes(), before)
        self.assertEqual((self.html.read_bytes(), self.vector.read_bytes()), original)

    def test_invalid_arguments_utf8_json_and_bounds_leave_output_absent(self):
        html, vector = self.html.read_bytes(), self.vector.read_bytes()
        cases = [({}, ["--id", "01"]), ({}, ["--source", ""]),
                 ({"html": b"\xff"}, []), ({"vector": b"not JSON"}, []),
                 ({"vector": b"[true]"}, []), ({"vector": b"[NaN]"}, []),
                 ({"vector": b"[1e39]"}, []), ({"html": b"x" * 6001}, []),
                 ({"html": b" " * (html_jsonl.MAX_INPUT_BYTES + 1)}, []),
                 ({"vector": b" " * (html_jsonl.MAX_VECTOR_BYTES + 1)}, [])]
        for content, extra in cases:
            with self.subTest(content=list(content), extra=extra):
                self.html.write_bytes(content.get("html", html))
                self.vector.write_bytes(content.get("vector", vector))
                self.adapter(*extra, success=False)
                self.assertFalse(self.output.exists())

    def test_installed_cli_import_export_preserves_text_full_id_and_f32_bits(self):
        repository = self.script.parents[2]
        binary = Path(os.environ.get("VECNOOK_BINARY", str(repository / "target/debug/vecnook")))
        binary = binary.resolve()
        self.assertTrue(binary.is_file(), f"Build the CLI or set VECNOOK_BINARY: {binary}")
        self.adapter()
        db = self.root / "imported"

        def native(*args):
            r = subprocess.run([str(binary), *map(str, args)], cwd=self.root, capture_output=True,
                               text=True, encoding="utf-8", timeout=30)
            self.assertEqual(r.returncode, 0, r.stderr)
            return r.stdout

        native("docs-import-jsonl", self.output, "--db", db)
        info = json.loads(native("docs-info", "--db", db))
        self.assertEqual(info["model"], "application-owned:model:revision")
        self.assertEqual((info["dimensions"], info["metric"]), (3, "squared_l2"))
        [row] = json.loads(native("docs-inspect", "--db", db))["documents"]
        self.assertEqual((row["id"], row["source"]), (str(2**64 - 1), self.html.name))
        self.assertEqual((row["start_line"], row["end_line"]), (1, 3))
        self.assertEqual(row["text"], "제목\n\r\n본문 & text")
        self.assertEqual(row["tags"], ["html", "fixture"])
        self.assertEqual(struct.pack("<3f", *row["vector"]), struct.pack("<3f", *self.coordinates))
        exported = self.root / "roundtrip.jsonl"
        native("docs-export-jsonl", "--db", db, "--output", exported)
        second = self.root / "second"
        native("docs-import-jsonl", exported, "--db", second)
        [restored] = json.loads(native("docs-inspect", "--db", second))["documents"]
        self.assertEqual(restored, row)


if __name__ == "__main__":
    unittest.main()
