"""WHAT: Sabotage policy evidence and require the independent oracle to reject it.
WHY: Missing observations, duplicate alerts or permission answers must go red.
ORACLE: Each corruption raises Finding for its own violated contract.
"""

import argparse
import unittest

from oracles import decision_path, judge_outcome as judge
from sentinel_oracles import judge_sentinel, judge_sentinel_feed, judge_sentinel_outcome
from transport import Finding, json_value
from binding_tests import AttributionControls, CourierControls, proposed_contract
from report import exit_code
from sentinel_raw_tests import SentinelRawControls, raw_outcome
from fixture_tests import NativeControls, IsolationControls, CoordinatorPhaseControls, VersionToolControls
from tls_tests import TLSControls, FailureFactControls
from native_version_tests import NativeVersionControls
from bounded_io_tests import DeadlineControls, ArchiveCollisionControls, AcquisitionControls, ProcessControls, HelperControls
from launch_prompt import judge as judge_launch_prompt
from launch_prompt_tests import LaunchPromptControls, answered_evidence, bound_contract


def notification_evidence():
    return {
        "case": "login", "instance_proven": True, "real_provider": True,
        "prompt_observed": True, "path": "heuristic", "precondition_n": 1,
        "outcome_n": 3, "expected": "notify", "notifications": 1,
        "route_proven": True, "dedup_barrier": True,
        "jev": "off", "heuristic_arm": [{"decided_by": "heuristic", "observation_id": 1}],
    }


def sentinel_evidence():
    return raw_outcome()


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
        from binding_tests import proposed_observations
        rows, expected = proposed_observations("fallback(timeout)")
        for row in rows:
            row["adapter_result"]["reason"] = "timeout"
        self.assertEqual(decision_path(rows, expected["tab"], "on", expected, proposed_contract()), "fallback_timeout")

    def test_cli_hint_before_json(self):
        self.assertEqual(json_value('hint\n{"tab_id":"opaque"}\n'), {"tab_id": "opaque"})

    def test_partial_json_is_red(self):
        with self.assertRaises(Finding):
            json_value('hint\n{"tab_id":')

    def test_positive_control(self):
        self.assertEqual(judge(notification_evidence()), "PASS")

    def test_sentinel_claim_without_receipt_is_red(self):
        with self.assertRaisesRegex(Finding, "receipts"):
            judge_sentinel_outcome(sentinel_evidence() | {"receipts": []})

    def test_sentinel_duplicate_transition_is_red(self):
        evidence = sentinel_evidence()
        evidence["journal"].append(evidence["journal"][1].copy())
        with self.assertRaisesRegex(Finding, "exactly one"):
            judge_sentinel_outcome(evidence)

    def test_sentinel_foreign_watch_is_red(self):
        evidence = sentinel_evidence()
        evidence["journal"][1]["sentinel_id"] = "foreign"
        with self.assertRaisesRegex(Finding, "foreign"):
            judge_sentinel_outcome(evidence)

    def test_sentinel_regex_model_call_is_red(self):
        with self.assertRaisesRegex(Finding, "called a model"):
            judge_sentinel_outcome(sentinel_evidence() | {"model_calls": 1})

    def test_sentinel_missing_completion_signal_is_red(self):
        with self.assertRaisesRegex(Finding, "completion"):
            judge_sentinel_outcome(sentinel_evidence() | {"probe_barrier": False})

    def test_sentinel_action_before_claim_is_red(self):
        evidence = sentinel_evidence()
        evidence["journal"].reverse()
        with self.assertRaisesRegex(Finding, "did not follow"):
            judge_sentinel_outcome(evidence)

    def test_sentinel_outcome_without_path_cannot_pass(self):
        evidence = sentinel_evidence()
        self.assertEqual(judge_sentinel_outcome(evidence), "OUTCOME VERIFIED")
        with self.assertRaisesRegex(Finding, "no journal evidence"):
            judge_sentinel(evidence)

    def test_sentinel_budget_route_is_red(self):
        evidence = sentinel_evidence() | {"variant": "budget-exhausted", "budget_reached": True}
        with self.assertRaisesRegex(Finding, "routed an action"):
            judge_sentinel_outcome(evidence)

    def test_persisted_sentinel_hidden_from_feed_is_red(self):
        with self.assertRaisesRegex(Finding, "absent from the activity feed"):
            judge_sentinel_feed({"instance_proven": True, "persisted_decision": True, "new_entries": []})


class ReportControls(unittest.TestCase):
    def contract(self):
        return {"decision": proposed_contract() | {"covered_points_status": "bound", "covered_points": [{"id": "fixture", "provider": "codex", "case": "permission", "status": "bound"}]},
                **{key: {"status": "bound"} for key in ("coordinator", "sentinel", "isolation", "update_fixture")}}

    def test_empty_report_is_red(self):
        self.assertEqual(exit_code({}, self.contract()), 1)

    def test_pending_covered_points_cannot_shrink_report(self):
        contract = self.contract()
        contract["decision"]["covered_points_status"] = "pending"
        self.assertEqual(exit_code({"PASS": 87, "N/A": 1}, contract), 1)

    def test_empty_point_list_cannot_shrink_report(self):
        contract = self.contract()
        contract["decision"]["covered_points"] = []
        self.assertEqual(exit_code({"PASS": 86, "N/A": 1}, contract), 1)

    def test_pending_isolation_cannot_pass(self):
        contract = self.contract()
        contract["isolation"]["status"] = "pending"
        self.assertEqual(exit_code({"PASS": 87, "N/A": 1}, contract), 1)

    def test_missing_base_cell_is_red(self):
        self.assertEqual(exit_code({"PASS": 85, "N/A": 1}, self.contract()), 1)
        self.assertEqual(exit_code({"PASS": 87, "N/A": 1}, self.contract(), True), 0)

    def test_missing_launch_matrix_keeps_report_red(self):
        self.assertEqual(exit_code({"PASS": 87, "N/A": 1}, self.contract()), 1)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--red-demo", action="store_true")
    parser.add_argument("--red-sentinel-demo", action="store_true")
    parser.add_argument("--red-binding-demo", action="store_true")
    parser.add_argument("--red-courier-demo", action="store_true")
    parser.add_argument("--red-sentinel-provenance-demo", action="store_true")
    parser.add_argument("--red-acquisition-demo", action="store_true")
    parser.add_argument("--red-launch-prompt-demo", choices=(
        "wrong-arm", "duplicate-observation", "missing-transcript", "non-trust-key",
        "unauthorized-off-answer"))
    options = parser.parse_args()
    if options.red_demo:
        evidence = notification_evidence() | {"notifications": 2}
        judge(evidence)
        return 0
    if options.red_sentinel_demo:
        evidence = sentinel_evidence()
        evidence["journal"].append(evidence["journal"][1].copy())
        judge_sentinel_outcome(evidence)
        return 0
    if options.red_binding_demo:
        from attribution import observations
        from binding_tests import proposed_observations
        rows, expected = proposed_observations()
        rows[1]["observation_id"] = rows[0]["observation_id"]
        observations(rows, expected, proposed_contract(), "on")
        return 0
    if options.red_courier_demo:
        from binding_tests import courier_fixture
        from coordinator_oracles import judge_coordinator_outcome
        judge_coordinator_outcome(courier_fixture() | {"version_receipts": []})
        return 0
    if options.red_sentinel_provenance_demo:
        from sentinel_raw import validate_raw
        evidence = sentinel_evidence()
        evidence["journal"][2]["recipient_receipt"] = "invented"
        validate_raw(evidence["journal"], evidence["raw_scope"])
        return 0
    if options.red_acquisition_demo:
        from acquisition_transport import AcquisitionProcess
        from bounded_io_tests import Clock, Flag, ImmediateThread
        from deadline_io import Deadline
        deadline = Deadline(10, Clock())
        lifecycle = AcquisitionProcess({"operation": deadline}, deadline, event_factory=Flag, thread_factory=ImmediateThread)
        lifecycle.finished.set()
        lifecycle.finish(cancel=False)
        return 0
    if options.red_launch_prompt_demo:
        evidence = answered_evidence()
        control = options.red_launch_prompt_demo
        if control == "wrong-arm":
            evidence["jev"] = "off"
        elif control == "duplicate-observation":
            evidence["journal_rows"][1]["observation_id"] = evidence["journal_rows"][0]["observation_id"]
        elif control == "missing-transcript":
            evidence["transcript"] = []
        elif control == "non-trust-key":
            evidence["transcript"][0]["value"]["kind"] = "permission"
        elif control == "unauthorized-off-answer":
            evidence["jev"] = "off"
            evidence["provenance_authorized"] = False
            evidence["provisioned_cwd"] = None
            evidence["journal_rows"][0]["decided_by"] = "fallback(no_adapter)"
            evidence["journal_rows"][1]["decided_by"] = "fallback(no_adapter)"
        try:
            judge_launch_prompt(evidence, bound_contract())
        except Finding as error:
            print(f"REJECTED {control}: {error}")
            return 1
        raise RuntimeError(f"Launch-prompt red demo {control} was not rejected")
    suite = unittest.TestSuite(unittest.defaultTestLoader.loadTestsFromTestCase(cls)
                               for cls in (Oracles, LaunchPromptControls, AttributionControls, CourierControls, ReportControls, SentinelRawControls,
                                           NativeControls, IsolationControls, CoordinatorPhaseControls, VersionToolControls,
                                           DeadlineControls, ArchiveCollisionControls, AcquisitionControls, ProcessControls,
                                           HelperControls, TLSControls, FailureFactControls, NativeVersionControls))
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main())
