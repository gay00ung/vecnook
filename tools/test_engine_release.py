"""Synthetic engine evidence only: guard against promotion with missing paired QA."""
import copy
import io
import json
from pathlib import Path
import tempfile
import tarfile
import unittest
from check_acceleration import ACCEL_CHECKS, acceleration_fingerprint, require_registry_lock
from check_engine_release import validate_acceleration
from check_acceleration import extract
from check_distribution import digest
from prepare_adapter_release import stage


class EngineReleaseTests(unittest.TestCase):
    def setUp(self):
        self.identity = dict(core_crate_sha256="core", core_source_sha256="core-source",
                             adapter_crate_sha256="adapter", adapter_source_sha256="adapter-source")
        self.rows = []
        for system in ["Linux", "Darwin", "Windows Server", "Windows 11"]:
            for rust in ["1.89.0", "stable"]:
                windows = None if "Windows" not in system else dict(caption=system, product_type=3 if system.endswith("Server") else 1, filesystem="NTFS")
                self.rows.append(dict(self.identity, schema_version=1, passed=True, registry=True, toolchain=rust,
                                      backend="Neon", checks=list(ACCEL_CHECKS), windows=windows,
                                      platform=dict(system="Windows" if windows else system, machine="ARM64")))
        self.soak = dict(self.identity, schema_version=1, completed=True, exit_code=0,
                         publisher_receipt_sha256="publisher", progress=dict(completed=True, elapsed_seconds=86400,
                         checked_records=100000, mapped_checks=3, generations_observed=3,
                         duration_gate_24h=True, records_gate_100k=True))

    def validate(self, rows=None, soak=None):
        return validate_acceleration("core", "core-source", "adapter", "adapter-source",
                                     self.rows if rows is None else rows, self.soak if soak is None else soak, "publisher")

    def test_all_eight_actual_hosts_and_both_packages_are_required(self):
        self.assertEqual(self.validate(), [])
        for index in range(8):
            self.assertTrue(self.validate(self.rows[:index]+self.rows[index+1:]))
        for field, value in [("core_crate_sha256", "wrong"), ("adapter_crate_sha256", "wrong"),
                             ("adapter_source_sha256", "wrong"), ("core_source_sha256", "wrong"),
                             ("registry", False), ("checks", []), ("backend", "assumed"), ("schema_version", True)]:
            rows = copy.deepcopy(self.rows)
            rows[0][field] = value
            self.assertTrue(self.validate(rows), field)
        for change in [lambda r: r[-1]["windows"].update(product_type=3),
                       lambda r: r[-1]["platform"].update(machine="x86_64"),
                       lambda r: r[-1]["windows"].update(filesystem="ReFS")]:
            rows = copy.deepcopy(self.rows)
            change(rows)
            self.assertTrue(self.validate(rows))

    def test_partial_short_boolean_unbound_or_single_generation_soak_never_passes(self):
        for field, value in [("elapsed_seconds", 86399.9), ("elapsed_seconds", float("nan")),
                             ("checked_records", 99999), ("checked_records", True), ("mapped_checks", 2),
                             ("generations_observed", 2), ("duration_gate_24h", False), ("completed", False)]:
            soak = copy.deepcopy(self.soak)
            soak["progress"][field] = value
            self.assertTrue(self.validate(soak=soak), field)
        for field, value in [("publisher_receipt_sha256", "running-receipt"), ("adapter_crate_sha256", "wrong"),
                             ("completed", False), ("exit_code", False), ("exit_code", 1)]:
            soak = copy.deepcopy(self.soak)
            soak[field] = value
            self.assertTrue(self.validate(soak=soak), field)

    def test_paired_promotion_normalizes_only_verified_release_identities(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "src").mkdir()
            (root / "src/lib.rs").write_text("pub fn api() {}\n")
            def settings(own, core, checksum):
                (root / "Cargo.toml.orig").write_text('[package]\nname="vecnook-accel"\nversion="'+own+'"\nrust-version="1.89"\n'
                    '[dependencies]\nvecnook={path="../..",version="='+core+'"}\n')
                (root / "Cargo.lock").write_text('version=4\n[[package]]\nname="vecnook-accel"\nversion="'+own+'"\n'
                    'dependencies=["vecnook"]\n[[package]]\nname="vecnook"\nversion="'+core+'"\n'
                    'source="registry+https://github.com/rust-lang/crates.io-index"\nchecksum="'+checksum+'"\n')
            settings("0.1.0-rc.1", "1.4.0-rc.1", "candidate-checksum")
            candidate = acceleration_fingerprint(root, "1.4.0-rc.1", "candidate-checksum")
            with self.assertRaises(ValueError):
                acceleration_fingerprint(root, "1.4.0-rc.1", "different-core")
            settings("0.1.0", "1.4.0", "stable-checksum")
            self.assertEqual(acceleration_fingerprint(root, "1.4.0", "stable-checksum"), candidate)
            (root / "README.md").write_text("Measured release notes\n")
            self.assertEqual(acceleration_fingerprint(root, "1.4.0", "stable-checksum"), candidate)
            (root / "src/lib.rs").write_text("pub fn api() { panic!() }\n")
            self.assertNotEqual(acceleration_fingerprint(root, "1.4.0", "stable-checksum"), candidate)
            with self.assertRaises(ValueError):
                require_registry_lock(root / "Cargo.lock", [("vecnook", "1.4.0", "different-checksum")])

    def test_review_stage_retains_source_and_refuses_changed_core(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            def archive(name, version, files, target):
                with tarfile.open(target, "w:gz") as package:
                    for path, text in files.items():
                        body = text.encode()
                        member = tarfile.TarInfo(f"{name}-{version}/"+path)
                        member.size = len(body)
                        package.addfile(member, io.BytesIO(body))
                return target
            def core(version, code="pub fn api() {}\n"):
                manifest = '[package]\nname="vecnook"\nversion="'+version+'"\n'
                return archive("vecnook", version, {"Cargo.toml": manifest, "Cargo.toml.orig": manifest,
                    "Cargo.lock": 'version=4\n[[package]]\nname="vecnook"\nversion="'+version+'"\n',
                    "src/lib.rs": code, "docs/demo-provenance.json": "{}\n"}, root / (version+".crate"))
            candidate = core("1.4.0-rc.1")
            stable = core("1.4.0")
            manifest = '[package]\nname="vecnook-accel"\nversion="0.1.0-rc.1"\n[dependencies]\nvecnook={version="=1.4.0-rc.1",path="../.."}\n'
            lock = 'version=4\n[[package]]\nname="vecnook-accel"\nversion="0.1.0-rc.1"\ndependencies=["vecnook"]\n[[package]]\nname="vecnook"\nversion="1.4.0-rc.1"\nsource="registry+https://github.com/rust-lang/crates.io-index"\nchecksum="'+digest(candidate)+'"\n'
            adapter = archive("vecnook-accel", "0.1.0-rc.1", {"Cargo.toml": manifest, "Cargo.toml.orig": manifest,
                "Cargo.lock": lock, "src/lib.rs": "pub fn mapped() {}\n"}, root / "adapter.crate")
            output = stage(candidate, adapter, stable, "0.1.0", root / "staged.crate")
            before = extract(adapter, root / "before", "vecnook-accel")
            after = extract(output, root / "after", "vecnook-accel")
            self.assertEqual(acceleration_fingerprint(before, "1.4.0-rc.1", digest(candidate)),
                             acceleration_fingerprint(after, "1.4.0", digest(stable)))
            with self.assertRaises(ValueError):
                stage(candidate, adapter, stable, "0.1.0", output)
            stable = core("1.4.0", "pub fn api() { panic!() }\n")
            with self.assertRaises(ValueError):
                stage(candidate, adapter, stable, "0.1.0", root / "invalid.crate")


if __name__ == "__main__":
    unittest.main()
