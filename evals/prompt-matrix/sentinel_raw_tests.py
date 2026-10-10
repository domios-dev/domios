"""WHAT: Reject wrong domains and fabricated Sentinel routing provenance.
WHY: A selected attempt and completed API are weaker than independent calls and delivery.
ORACLE: Only the captured raw schema survives; external counts and receipts stay mandatory.
"""

import copy
import unittest

from sentinel_raw import validate_raw
from sentinel_oracles import bucket_label, judge_sentinel_outcome, judge_sentinel_feed
from transport import Finding


EVALUATION = "3cbe2a1f-952d-4652-93de-43c98af4e097"
SCOPE = {"sentinel_id": "own-watch", "tab": "own-creator", "team_id": None, "workspace_id": "own-workspace"}


def raw_transition():
    common = SCOPE | {"time": "opaque-time", "kind": "sentinel", "state_hash": "probe-domain-hash",
                      "evaluation_id": EVALUATION, "observation_id": EVALUATION,
                      "classification_source": "regex", "classification_reason": "transition",
                      "label": bucket_label("success"), "confidence": 1.0, "route": "send",
                      "route_target": "intended-tab", "recipient_receipt": None, "asked": False,
                      "budget": {}}
    return [common | {"stage": stage, "route_status": status, "action": action, "reason": "transition"}
            for stage, status, action in (("decision", "not_attempted", "decision"),
                                         ("route_claim", "claimed", "route_claim"),
                                         ("route_completed", "api_completed", "send"))]


def raw_outcome():
    return {"case": "sentinel", "variant": "fake-ci", "instance_proven": True,
            "sentinel_id": SCOPE["sentinel_id"], "creator": SCOPE["tab"], "raw_scope": dict(SCOPE),
            "probe_barrier": True, "journal": raw_transition(), "expected_buckets": ["success"],
            "receipts": ["success"], "route_action": "send", "unchanged_actions": 0,
            "regex_mode": True, "model_calls": 0}


class SentinelRawControls(unittest.TestCase):
    def test_captured_transition_positive(self):
        self.assertEqual(len(validate_raw(raw_transition(), SCOPE)[EVALUATION]), 3)

    def test_foreign_scope_is_red(self):
        for key in SCOPE:
            with self.subTest(key=key):
                rows = raw_transition()
                rows[1][key] = "foreign"
                with self.assertRaisesRegex(Finding, "foreign captured scope"):
                    validate_raw(rows, SCOPE)

    def test_numeric_observation_is_not_sentinel_uuid(self):
        rows = raw_transition()
        rows[0]["observation_id"] = 1
        with self.assertRaisesRegex(Finding, "selected attempt"):
            validate_raw(rows, SCOPE)

    def test_attempt_is_not_physical_call(self):
        evidence = raw_outcome() | {"jev": "on", "regex_mode": False, "model_calls": 0}
        for row in evidence["journal"]:
            row["classification_source"] = "adapter"
        with self.assertRaisesRegex(Finding, "exact model request count"):
            judge_sentinel_outcome(evidence)

    def test_completed_api_is_not_recipient_receipt(self):
        with self.assertRaisesRegex(Finding, "receipts"):
            judge_sentinel_outcome(raw_outcome() | {"receipts": []})

    def test_raw_recipient_cannot_be_populated(self):
        rows = raw_transition()
        rows[2]["recipient_receipt"] = "invented"
        with self.assertRaisesRegex(Finding, "invented a recipient receipt"):
            validate_raw(rows, SCOPE)

    def test_request_json_hash_cannot_replace_probe_hash(self):
        rows = raw_transition()
        rows[2]["state_hash"] = "request-json-domain-hash"
        with self.assertRaisesRegex(Finding, "changed captured scope/hash"):
            validate_raw(rows, SCOPE)

    def test_equal_request_hash_does_not_create_call_receipt(self):
        evidence = raw_outcome() | {"jev": "on", "regex_mode": False, "model_calls": 0,
                                    "request_json_hash": "probe-domain-hash"}
        for row in evidence["journal"]:
            row["classification_source"] = "adapter"
        with self.assertRaisesRegex(Finding, "exact model request count"):
            judge_sentinel_outcome(evidence)

    def test_wrong_stage_status_is_red(self):
        rows = raw_transition()
        rows[2]["route_status"] = "delivered"
        with self.assertRaisesRegex(Finding, "stage/route status"):
            validate_raw(rows, SCOPE)

    def test_unchanged_has_no_attempt_or_route(self):
        row = raw_transition()[0] | {"classification_reason": "unchanged"}
        with self.assertRaisesRegex(Finding, "Unchanged evaluation"):
            validate_raw([row], SCOPE)

    def test_unchanged_requires_fresh_evaluation(self):
        rows = raw_transition()
        unchanged = rows[0] | {"classification_source": "not_attempted", "classification_reason": "unchanged",
                              "reason": "unchanged", "observation_id": None, "label": None,
                              "route": None, "route_target": None}
        with self.assertRaisesRegex(Finding, "changed captured scope/hash"):
            validate_raw(rows + [unchanged], SCOPE)

    def test_failed_probe_cannot_retain_hash(self):
        row = raw_transition()[0] | {"stage": "skip", "route_status": "not_attempted", "action": "skip",
                                    "reason": "probe_failed", "classification_reason": "probe_failed",
                                    "classification_source": "not_attempted", "observation_id": None,
                                    "label": None, "route": None, "route_target": None}
        with self.assertRaisesRegex(Finding, "Failed probe retained"):
            validate_raw([row], SCOPE)
        self.assertEqual(len(validate_raw([row | {"state_hash": None}], SCOPE)), 1)

    def test_failed_route_preserves_transition(self):
        rows = raw_transition()
        rows[2].update(stage="route_failed", route_status="failed", action="skip", reason="route_failed")
        self.assertEqual(len(validate_raw(rows, SCOPE)[EVALUATION]), 3)
        rows[2]["classification_reason"] = "route_failed"
        with self.assertRaisesRegex(Finding, "captured confident transition"):
            validate_raw(rows, SCOPE)

    def test_disconnected_has_no_observation_id(self):
        row = raw_transition()[0] | {"classification_source": "disconnected", "classification_reason": "adapter_disconnected",
                                    "reason": "adapter_disconnected", "label": None, "route": None, "route_target": None}
        with self.assertRaisesRegex(Finding, "selected attempt"):
            validate_raw([row], SCOPE)
        self.assertEqual(len(validate_raw([row | {"observation_id": None}], SCOPE)), 1)

    def test_duplicate_stage_is_red(self):
        rows = raw_transition()
        with self.assertRaisesRegex(Finding, "Duplicate Sentinel stage"):
            validate_raw(rows + [copy.deepcopy(rows[1])], SCOPE)

    def test_public_feed_cannot_supply_raw_uuid_binding(self):
        row = {key: None for key in ("time", "team_id", "label", "confidence_band", "action", "reason", "budget_reached", "fallback")}
        row.update(kind="sentinel", observation_id=EVALUATION)
        with self.assertRaisesRegex(Finding, "cannot substitute raw Sentinel UUID"):
            judge_sentinel_feed({"instance_proven": True, "persisted_decision": True, "new_entries": [row]})
