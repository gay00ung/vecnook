"""Guard against false retained-session/quality evidence in the CLI collector."""
import struct
import tempfile
import unittest
from pathlib import Path

from compare_cli_sessions import canonical, decode, validate_ready, vectors


class EvidenceContracts(unittest.TestCase):
    def test_reinitializing_or_unidentified_process_is_not_a_session(self):
        for ready in ({}, {"event": "ready", "snapshot_opens": 100, "sq8_builds": 100},
                      {"event": "ready", "snapshot_opens": 1, "sq8_builds": 2}):
            with self.assertRaises(ValueError):
                validate_ready(ready, False)
        validate_ready({"event": "ready", "snapshot_opens": 1, "sq8_builds": 1}, False)
        with self.assertRaises(ValueError):
            validate_ready({"event": "ready", "snapshot_opens": 1, "sq8_builds": 1}, True)

    def test_truncated_fixture_is_not_silently_a_smaller_workload(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "queries.fvecs"
            path.write_bytes((struct.pack("<If", 1, 1.0) * 20) + struct.pack("<I", 2) + struct.pack("<f", 1.0))
            with self.assertRaises(ValueError):
                vectors(path)

    def test_bitwise_comparison_retains_signed_zero_and_full_ids(self):
        report = dict(generation="1", sequence="0", approximate=True, complete=True,
                      eligible_count=1, original_distance_computations=1,
                      neighbors=[dict(id="18446744073709551615", distance=-0.0, metadata="원문")])
        before = canonical(report)
        report["neighbors"][0]["distance"] = 0.0
        self.assertNotEqual(before, canonical(report))
        self.assertEqual(struct.pack("<d",decode('{"distance":-0}')["distance"]),struct.pack("<d",-0.0))


if __name__ == "__main__":
    unittest.main()
