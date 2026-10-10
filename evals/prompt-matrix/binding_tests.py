"""WHAT: Exercise missing, foreign and contradictory attribution/receipt inputs.
WHY: A route, configured adapter or same-tab id must never manufacture a pass.
ORACLE: Synthetic controls pass only at the validator seam; each corruption raises its named predicate.
"""

from copy import deepcopy
import unittest

from attribution import IDENTITY, REQUIRED, observations
from coordinator_oracles import judge_coordinator_outcome
from transport import Finding, Pending


def proposed_contract():
    return {"status": "bound", "fields": {key: key for key in REQUIRED}}


def proposed_observations(source="adapter"):
    expected = dict(zip(IDENTITY, ("own", "launch", "session", "occurrence", "point")))
    rows = [expected | {"observation_id": identifier, "completed": True, "decided_by": source,
                        "heuristic_verdict": "heuristic-token", "selected_verdict": "adapter-token",
                        "adapter_result": {"status": "verdict", "verdict": "adapter-token"}}
            for identifier in (11, 12)]
    if source != "adapter":
        for row in rows:
            row["selected_verdict"] = row["heuristic_verdict"]
            row["adapter_result"] = {"status": "fallback", "reason": "no_adapter"}
    return rows, expected


def courier_fixture(variant="default"):
    common = {"provider": "codex", "target_version": "target"}
    rows = [common | {"label": "update_started"},
            common | {"tab": "own", "target_version": None, "label": "safe_choice_answered"},
            common | {"label": "update_failed" if variant == "fail" else "update_succeeded"}]
    if variant != "fail":
        rows += [common | {"tab": "own", "label": "restart_requested"},
                 common | {"tab": "own", "label": "resumed"}]
    return {"instance_proven": True, "real_prompt": True, "isolated_update_scope": True,
            "provider": "codex", "target_version": "target", "variant": variant, "journal": rows,
            "persisted_run": {"summary_id": "notice"}, "tabs": ["own"], "prompt_tabs": ["own"],
            "owner_notices": [{"id": "notice", "level": "failure" if variant == "fail" else "verbose"}],
            "safe_receipts": [{"tab": "own", "occurrence": "prompt", "provider_ready": True, "choice": "safe-token"}],
            "safe_choice": "safe-token", "prompt_occurrences": {"own": "prompt"},
            "sessions": {"own": "session"},
            "restart_launches": [] if variant == "fail" else [{"tab": "own", "launch": "new", "session": "session"}],
            "version_receipts": [] if variant == "fail" else [{"tab": "own", "launch": "new", "session": "session",
                "version": "target", "source": "own_session_tool_result", "exit_code": 0}],
            "failure_barrier": True, "busy_tab": "own", "install_n": 3, "busy_restart_n": 5,
            "busy_checkpoint": {"tab": "own", "install_completed": True, "active_turn": True, "restart_count": 0},
            "idle_receipt": {"tab": "own", "session": "session", "active_turn": False, "active_attempt": False, "n": 4}}


class AttributionControls(unittest.TestCase):
    def reject(self, change, message, jev="on", disagreement=False):
        rows, expected = proposed_observations()
        change(rows)
        with self.assertRaisesRegex(Finding, message):
            observations(rows, expected, proposed_contract(), jev, disagreement=disagreement)

    def test_actual_partial_producer_schema_is_pending(self):
        contract = {"status": "bound", "fields": {"tab": "tab", "decided_by": "decided_by", "observation_id": "observation_id"}}
        with self.assertRaisesRegex(Pending, "schema is not bound"):
            observations([], {}, contract, "on")

    def test_missing_expected_occurrence_is_red(self):
        rows, expected = proposed_observations()
        del expected["occurrence"]
        with self.assertRaisesRegex(Finding, "Expected decision identity"):
            observations(rows, expected, proposed_contract(), "on")

    def test_wrong_point_is_red(self):
        self.reject(lambda rows: rows[0].update(point="other"), "wrong point")

    def test_wrong_session_is_red(self):
        self.reject(lambda rows: rows[0].update(session="other"), "wrong point")

    def test_wrong_launch_is_red(self):
        self.reject(lambda rows: rows[0].update(launch="other"), "wrong point")

    def test_wrong_occurrence_is_red(self):
        self.reject(lambda rows: rows[0].update(occurrence="other"), "wrong point")

    def test_foreign_tab_cannot_supply_barrier(self):
        self.reject(lambda rows: rows[0].update(tab="other"), "Two distinct")

    def test_duplicate_id_is_red(self):
        self.reject(lambda rows: rows[1].update(observation_id=11), "Duplicate or stale")

    def test_stale_id_is_red(self):
        self.reject(lambda rows: rows[1].update(observation_id=10), "Duplicate or stale")

    def test_zero_id_is_red(self):
        self.reject(lambda rows: rows[0].update(observation_id=0), "missing or stale")

    def test_uncompleted_id_is_red(self):
        self.reject(lambda rows: rows[0].update(completed=False), "not completed")

    def test_wrong_selected_arm_is_red(self):
        self.reject(lambda rows: rows[0].update(selected_verdict="heuristic-token"), "deciding adapter arm")

    def test_disconnected_adapter_is_red(self):
        self.reject(lambda rows: None, "wrong deciding arm", jev="off")

    def test_missing_paired_arm_is_red(self):
        self.reject(lambda rows: rows[0].update(heuristic_verdict=None), "Paired heuristic")

    def test_equal_verdicts_are_not_disagreement(self):
        self.reject(lambda rows: rows[0].update(heuristic_verdict="adapter-token"), "disagreement", disagreement=True)

    def test_actual_disagreement_control(self):
        rows, expected = proposed_observations()
        self.assertEqual(len(observations(rows, expected, proposed_contract(), "on", disagreement=True)), 2)

    def test_no_adapter_control(self):
        rows, expected = proposed_observations("fallback(no_adapter)")
        self.assertEqual(observations(rows, expected, proposed_contract(), "off")[-1]["path"], "fallback_no_adapter")

    def test_fallback_reason_mismatch_is_red(self):
        rows, expected = proposed_observations("fallback(no_adapter)")
        rows[0]["adapter_result"]["reason"] = "timeout"
        with self.assertRaisesRegex(Finding, "fallback reason"):
            observations(rows, expected, proposed_contract(), "on")


class CourierControls(unittest.TestCase):
    def reject(self, change, message, variant="default"):
        evidence = courier_fixture(variant)
        change(evidence)
        with self.assertRaisesRegex(Finding, message):
            judge_coordinator_outcome(evidence)

    def test_one_run_control(self):
        self.assertEqual(judge_coordinator_outcome(courier_fixture()), "OUTCOME VERIFIED")

    def test_busy_control(self):
        self.assertEqual(judge_coordinator_outcome(courier_fixture("busy")), "OUTCOME VERIFIED")

    def test_failure_control(self):
        self.assertEqual(judge_coordinator_outcome(courier_fixture("fail")), "OUTCOME VERIFIED")

    def test_duplicate_install_is_red(self):
        self.reject(lambda e: e["journal"].insert(1, deepcopy(e["journal"][0])), "exactly one provider update")

    def test_claim_without_recipient_is_red(self):
        self.reject(lambda e: e.update(version_receipts=[]), "independent launch/version")

    def test_foreign_session_version_is_red(self):
        self.reject(lambda e: e["version_receipts"][0].update(session="foreign"), "Version receipt")

    def test_global_version_is_red(self):
        self.reject(lambda e: e["version_receipts"][0].update(source="global_version"), "Version receipt")

    def test_old_version_is_red(self):
        self.reject(lambda e: e["version_receipts"][0].update(version="old"), "Version receipt")

    def test_failure_restart_is_red(self):
        self.reject(lambda e: e["journal"].append({"provider": "codex", "target_version": "target", "tab": "own", "label": "restart_requested"}), "Failed update restarted", "fail")

    def test_duplicate_owner_notice_is_red(self):
        self.reject(lambda e: e["owner_notices"].append(e["owner_notices"][0].copy()), "exactly one observable owner")

    def test_safe_claim_without_receipt_is_red(self):
        self.reject(lambda e: e.update(safe_receipts=[]), "prompt disappearance receipts")

    def test_safe_wrong_occurrence_is_red(self):
        self.reject(lambda e: e["safe_receipts"][0].update(occurrence="old"), "wrong prompt occurrence")

    def test_busy_restart_before_idle_is_red(self):
        self.reject(lambda e: e.update(busy_restart_n=3), "eligible idle", "busy")

    def test_busy_install_requires_active_checkpoint(self):
        self.reject(lambda e: e["busy_checkpoint"].update(active_turn=False), "Busy checkpoint", "busy")

    def test_failure_requires_completion_barrier(self):
        self.reject(lambda e: e.update(failure_barrier=False), "post-failure", "fail")
