"""WHAT: Sabotage policy evidence and require the independent oracle to reject it.
WHY: Missing observations, duplicate alerts or permission answers must go red.
ORACLE: Each corruption raises Finding for its own violated contract.
"""

import argparse
import unittest

from oracles import decision_path, judge
from transport import Finding, json_value


def notification_evidence():
    return {
        "case": "login", "instance_proven": True, "real_provider": True,
        "prompt_observed": True, "path": "heuristic", "precondition_n": 1,
        "outcome_n": 3, "expected": "notify", "notifications": 1,
        "route_proven": True, "dedup_barrier": True,
        "jev": "off", "heuristic_arm": [{"decided_by": "heuristic", "observation_id": 1}],
    }


class Oracles(unittest.TestCase):
    def test_duplicate_alert_is_red(self):
        evidence = notification_evidence()
        evidence["notifications"] = 2
        with self.assertRaisesRegex(Finding, "exactly one"):
            judge(evidence)

    def test_setup_without_prompt_is_red(self):
        evidence = notification_evidence()
        evidence["prompt_observed"] = False
        with self.assertRaisesRegex(Finding, "never observed"):
            judge(evidence)

    def test_answered_permission_is_red(self):
        evidence = notification_evidence() | {
            "case": "permission", "answers": 1, "pending_same": True,
            "tool_completed": False, "observation_barrier": True,
        }
        with self.assertRaisesRegex(Finding, "was answered"):
            judge(evidence)

    def test_uncommitted_needed_answer_is_red(self):
        evidence = notification_evidence() | {
            "expected": "answer", "answer_committed": False, "far_end": True,
        }
        with self.assertRaisesRegex(Finding, "not committed"):
            judge(evidence)

    def test_wrong_order_is_red(self):
        evidence = notification_evidence() | {"outcome_n": 1}
        with self.assertRaisesRegex(Finding, "did not follow"):
            judge(evidence)

    def test_no_duplicate_barrier_is_red(self):
        evidence = notification_evidence() | {"dedup_barrier": False}
        with self.assertRaisesRegex(Finding, "not exercised"):
            judge(evidence)

    def test_missing_path_is_red(self):
        evidence = notification_evidence() | {"path": None}
        with self.assertRaisesRegex(Finding, "no journal evidence"):
            judge(evidence)

    def test_connected_status_does_not_infer_path(self):
        self.assertIsNone(decision_path([{"tab": "own", "action": "strong_action"}], "own", "on"))

    def test_foreign_observation_does_not_decide(self):
        rows = [{"tab": "foreign", "decided_by": "adapter", "observation_id": 1}]
        self.assertIsNone(decision_path(rows, "own", "on"))

    def test_fallback_reason_round_trip(self):
        rows = [{"tab": "own", "decided_by": "fallback(timeout)", "observation_id": 1}]
        self.assertEqual(decision_path(rows, "own", "on"), "fallback_timeout")

    def test_cli_hint_before_json(self):
        self.assertEqual(json_value('hint\n{"tab_id":"opaque"}\n'), {"tab_id": "opaque"})

    def test_partial_json_is_red(self):
        with self.assertRaises(Finding):
            json_value('hint\n{"tab_id":')

    def test_positive_control(self):
        self.assertEqual(judge(notification_evidence()), "PASS")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--red-demo", action="store_true")
    options = parser.parse_args()
    if options.red_demo:
        evidence = notification_evidence() | {"notifications": 2}
        judge(evidence)
        return 0
    result = unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(Oracles))
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main())
