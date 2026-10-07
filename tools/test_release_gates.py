"""Contract tests for stable publication evidence; synthetic receipts are never release evidence."""
import copy
from pathlib import Path
import tempfile
import unittest
from check_distribution import fingerprint
from check_release import CHECKS, validate


class ReleaseGateTests(unittest.TestCase):
    def setUp(self):
        self.rows = [dict(schema_version=1, passed=True, registry=True, crate_sha256="crate", source_sha256="source",
                          platform=dict(system=os), toolchain=rust, checks=list(CHECKS), windows=None)
                     for os in ["Linux", "Darwin", "Windows"] for rust in ["1.89.0", "stable"]]
        self.rows[-1]["windows"] = dict(caption="Microsoft Windows 11 Pro", product_type=1, filesystem="NTFS")
        self.soak = dict(schema_version=1, completed=True, exit_code=0, crate_sha256="crate", source_sha256="source",
                         progress=dict(completed=True, elapsed_seconds=86400, ack_mutations=100000, duration_gate_24h=True,
                                       mutation_gate_100k=True, restarts=3, backups_verified=1))

    def test_required_evidence_boundaries(self):
        self.assertEqual(validate("crate", "source", self.rows, self.soak), [])
        for field, value in [("elapsed_seconds", 86399.99), ("elapsed_seconds", float("nan")),
                             ("ack_mutations", 99999), ("ack_mutations", True), ("completed", False),
                             ("backups_verified", 0), ("restarts", 2)]:
            soak = copy.deepcopy(self.soak)
            soak["progress"][field] = value
            self.assertTrue(validate("crate", "source", self.rows, soak), field)

    def test_server_ci_wrong_artifacts_or_missing_consumer_checks_cannot_pass(self):
        for modification in [lambda rows: rows[-1].update(windows=dict(caption="Microsoft Windows Server 2025", product_type=3, filesystem="NTFS")),
                             lambda rows: rows[0].update(crate_sha256="different"),
                             lambda rows: rows[0].update(source_sha256="different"),
                             lambda rows: rows[0].update(registry=False),
                             lambda rows: rows[0].update(checks=[])]:
            rows = copy.deepcopy(self.rows)
            modification(rows)
            self.assertTrue(validate("crate", "source", rows, self.soak))
        self.assertTrue(validate("crate", "source", self.rows[:-1], self.soak))
        soak = copy.deepcopy(self.soak)
        soak.update(completed=False)
        self.assertTrue(validate("crate", "source", self.rows, soak))

    def test_receipts_reject_boolean_numbers_and_wrong_soak_identity(self):
        for field, value in [("schema_version", True), ("exit_code", False), ("crate_sha256", "wrong"),
                             ("source_sha256", "wrong")]:
            soak = copy.deepcopy(self.soak)
            soak[field] = value
            self.assertTrue(validate("crate", "source", self.rows, soak), field)
        rows = copy.deepcopy(self.rows)
        rows[-1]["windows"]["product_type"] = True
        self.assertTrue(validate("crate", "source", rows, self.soak))

    def test_source_binding_allows_version_docs_only(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            files = {"src/lib.rs": b"pub fn api() {}\n", "tests/contract.rs": b"// contract\n",
                     "tools/check.py": b"# check\n", "examples/documents/sample/a.md": b"authored text\n",
                     "docs/demo-provenance.json": b"{}\n"}
            for name, content in files.items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(content)
            manifest = '[package]\nname="vecnook"\nversion="VERSION"\nrust-version="1.89"\n[dependencies]\n'
            lock = 'version=4\n[[package]]\nname="vecnook"\nversion="VERSION"\n'
            for name, text in [("Cargo.toml.orig", manifest), ("Cargo.lock", lock)]:
                (root / name).write_text(text.replace("VERSION", "1.0.0-rc.1"))
            original = fingerprint(root)
            for name, text in [("Cargo.toml.orig", manifest), ("Cargo.lock", lock)]:
                (root / name).write_text(text.replace("VERSION", "1.0.0"))
            (root / "README.md").write_text("Updated stable installation instructions\n")
            (root / "docs/releasing.md").write_text("Checked release notes\n")
            self.assertEqual(fingerprint(root), original)
            for name, content in files.items():
                (root / name).write_bytes(content+b"changed\n")
                self.assertNotEqual(fingerprint(root), original, name)
                (root / name).write_bytes(content)
            (root / "Cargo.toml.orig").write_text(manifest.replace("VERSION", "1.0.0").replace('"1.89"', '"1.90"'))
            self.assertNotEqual(fingerprint(root), original)


if __name__ == "__main__":
    unittest.main()
