"""WHAT: Validate prompt, journal-arm, provenance and cron completion evidence.
WHY: A green cell must bind a real provider prompt to one authorized far-end result.
ORACLE: Each deliberate evidence corruption raises Finding at its named predicate.
"""

import unittest

from launch_prompt import REQUIRED_BINDINGS, judge
from transport import Finding, Pending


def bound_contract():
    return {
        "status": "bound",
        "owner_decision": {"status": "recorded", "choice": "A", "stride": "D2", "rule": "fixture"},
        "bindings": {
            field: {"status": "bound", "source": f"fixture::{field}", "field": field}
            for field in REQUIRED_BINDINGS
        },
    }


def answered_evidence():
    return {
        "provider": "codex",
        "folder": "owner-dir",
        "jev": "on",
        "status": "finished",
        "cwd": "/fixture/owner-dir",
        "expected_cwd": "/fixture/owner-dir",
        "provisioned_cwd": "/fixture/owner-dir",
        "provenance_authorized": True,
        "transcript_path": "/receipt/transcript.jsonl",
        "transcript": [{
            "kind": "launch_prompt_capture",
            "value": {
                "tab_id": "tab-1",
                "provider": "codex",
                "kind": "directory_trust",
                "prompt": {"question": "opaque prompt", "options": [
                    {"label": "option A", "value": "trusted"},
                    {"label": "option B", "value": "untrusted"},
                ]},
                "screen": "opaque provider screen",
            },
        }],
        "tab_id": "tab-1",
        "journal_rows": [
            {"tab": "tab-1", "action": "unattended_launch_prompt", "phase": "unattended_model",
             "decided_by": "adapter", "observation_id": 10, "reason": "declared_choice"},
            {"tab": "tab-1", "action": "unattended_launch_prompt", "phase": "unattended_model",
             "decided_by": "adapter", "observation_id": 11, "reason": "declared_choice"},
            {"tab": "tab-1", "action": "recovery", "phase": "action_authorized",
             "decided_by": "adapter", "observation_id": 11,
             "reason": "provider_safe_step_authorized"},
        ],
        "prompt_cleared": True,
        "session_authenticated": True,
        "provider_session_id": "session-1",
        "role_marker": True,
        "owner_notices": 0,
        "run_id": "run-1",
        "cron_run": {"run_id": "run-1", "session_id": "session-1",
                     "status": "Success", "finished_at": 2},
    }


class LaunchPromptControls(unittest.TestCase):
    def test_adapter_answer_reaches_successful_cron_far_end(self):
        self.assertEqual(judge(answered_evidence(), bound_contract()),
                         {"outcome": "answer", "decided_by": "adapter"})

    def test_disconnected_answer_uses_no_adapter_fallback(self):
        evidence = answered_evidence()
        evidence["jev"] = "off"
        evidence["journal_rows"][0]["decided_by"] = "fallback(no_adapter)"
        evidence["journal_rows"][1]["decided_by"] = "fallback(no_adapter)"
        evidence["journal_rows"].pop()
        evidence["journal_rows"].append({"tab": "tab-1", "reason": "deterministic_directory_trust"})
        self.assertEqual(judge(evidence, bound_contract()),
                         {"outcome": "answer", "decided_by": "fallback(no_adapter)"})

    def test_unprovisioned_prompt_stays_pending_for_owner(self):
        evidence = answered_evidence()
        evidence.update(provisioned_cwd=None, provenance_authorized=False,
                        prompt_cleared=False, owner_notices=1)
        evidence["status"] = "AwaitingOwnerPrompt"
        evidence["cron_run"] = {"run_id": "run-1", "status": "AwaitingOwnerPrompt"}
        evidence["journal_rows"] = evidence["journal_rows"][:1]
        evidence["journal_rows"][0]["decided_by"] = "fallback(no_adapter)"
        evidence["jev"] = "off"
        self.assertEqual(judge(evidence, bound_contract()),
                         {"outcome": "owner_notice", "decided_by": "fallback(no_adapter)"})

    def test_non_trust_prompt_stays_pending_without_a_key(self):
        evidence = answered_evidence()
        evidence.update(status="AwaitingOwnerPrompt", prompt_cleared=False, owner_notices=1)
        evidence["transcript"][0]["value"]["kind"] = "permission"
        evidence["journal_rows"] = evidence["journal_rows"][:2]
        evidence["cron_run"] = {"run_id": "run-1", "status": "AwaitingOwnerPrompt"}
        self.assertEqual(judge(evidence, bound_contract()),
                         {"outcome": "owner_notice", "decided_by": "adapter"})

    def test_wrong_arm_is_red(self):
        evidence = answered_evidence()
        evidence["jev"] = "off"
        with self.assertRaisesRegex(Finding, "wrong deciding arm"):
            judge(evidence, bound_contract())

    def test_duplicate_observation_id_is_red(self):
        evidence = answered_evidence()
        evidence["journal_rows"][1]["observation_id"] = 10
        with self.assertRaisesRegex(Finding, "two distinct observation ids"):
            judge(evidence, bound_contract())

    def test_missing_transcript_is_red(self):
        evidence = answered_evidence()
        evidence["transcript"] = []
        with self.assertRaisesRegex(Finding, "Missing transcript records"):
            judge(evidence, bound_contract())

    def test_non_trust_prompt_key_is_red(self):
        evidence = answered_evidence()
        evidence["transcript"][0]["value"]["kind"] = "permission"
        with self.assertRaisesRegex(Finding, "non-trust prompt"):
            judge(evidence, bound_contract())

    def test_jev_off_answer_without_provenance_is_red(self):
        evidence = answered_evidence()
        evidence["jev"] = "off"
        evidence["provenance_authorized"] = False
        evidence["provisioned_cwd"] = None
        evidence["journal_rows"][0]["decided_by"] = "fallback(no_adapter)"
        evidence["journal_rows"][1]["decided_by"] = "fallback(no_adapter)"
        with self.assertRaisesRegex(Finding, "without launch provenance"):
            judge(evidence, bound_contract())

    def test_pending_source_binding_stays_pending(self):
        contract = bound_contract()
        contract["bindings"]["action_phase"]["status"] = "pending"
        with self.assertRaisesRegex(Pending, "Unbound launch-prompt field action_phase"):
            judge(answered_evidence(), contract)
