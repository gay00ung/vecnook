import unittest
from run_session_soak import duration_gates


class SessionSoakGateTests(unittest.TestCase):
    def valid(self):
        return dict(elapsed_seconds=86400.1, verified_requests=100000, reference_queries=100,
                    session_starts=8, released_cache_leases=8, session_sq8_builds=0,
                    normal_closes=2, eof_closes=2, forced_kills=2, idle_exits=2,
                    invalid_requests_recovered=16, active_lease_refusals=8, cache_preparations=8)

    def test_actual_duration_and_each_lifecycle_are_required(self):
        self.assertTrue(duration_gates(self.valid()))
        for key, value in [("elapsed_seconds", 86399.99), ("verified_requests", 99999),
                           ("reference_queries", 4), ("session_starts", 3),
                           ("released_cache_leases", 7), ("session_sq8_builds", 1),
                           ("normal_closes", 0), ("eof_closes", 0), ("forced_kills", 0),
                           ("idle_exits", 0), ("invalid_requests_recovered", 15),
                           ("active_lease_refusals", 7), ("cache_preparations", 7),
                           ("normal_closes", 3)]:
            sample = self.valid()
            sample[key] = value
            self.assertFalse(duration_gates(sample), key)

    def test_short_or_missing_progress_cannot_approve(self):
        self.assertFalse(duration_gates({}))
        progress = self.valid()
        progress["elapsed_seconds"] = 2
        progress["verified_requests"] = 1000000
        self.assertFalse(duration_gates(progress))
        for value in [float("inf"), float("nan"), True, "86400"]:
            progress = self.valid()
            progress["elapsed_seconds"] = value
            self.assertFalse(duration_gates(progress))
        progress = self.valid()
        progress["session_starts"] = 8.0
        self.assertFalse(duration_gates(progress))


if __name__ == "__main__":
    unittest.main()
