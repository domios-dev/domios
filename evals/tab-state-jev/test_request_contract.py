"""WHAT: immutable bytes, declared questions, schema and qualification boundaries.
WHY: historical or synthetic inputs cannot establish current runtime parity.
ORACLE: byte/domain hashes and exact supplied answer sets; qualification stays off.
Synthetic mechanism fixtures; no current event or private-screen fixtures.
"""

from dataclasses import FrozenInstanceError
import hashlib
import json
from pathlib import Path
import unittest
from unittest.mock import patch

import request_contract as c
import run


class ContractTests(unittest.TestCase):
    def setUp(self):
        self.criteria_raw = Path(__file__).with_name("criteria.json").read_bytes()
        self.criteria = c.load_criteria_bytes(self.criteria_raw)
        self.questions = {"label": {"type": "choice", "instructions": "opaque", "criteria": self.criteria},
                          "prompt_visible": {"type": "noul", "instructions": "opaque",
                                             "criteria": {"true": "opaque-one", "false": "opaque-two"}}}

    def manifest(self, questions=None):
        return c.Manifest("a" * 40, self.criteria_raw, b"synthetic mechanism bytes\n",
                          c.encode(self.questions if questions is None else questions), c.ENDPOINT, "fixture-alias")

    def answer(self):
        return {"answers": {"label": {"type": "choice", "choice": "ready_idle",
                                     "probabilities": {"ready_idle": 0.95}},
                            "prompt_visible": {"type": "noul", "noul": 0.1}}}

    def test_canonical_round_trip_exact_supplied_bytes(self):
        self.assertEqual(self.criteria, json.loads(self.criteria_raw))
        self.assertEqual(c.sha256(self.criteria_raw), c.CANONICAL_SHA256)

    def test_historical_variants_remain_missing_other_and_bytes_unchanged(self):
        path = Path(__file__).with_name("criteria_variants.json")
        before = path.read_bytes()
        for variant in ("current", "auth-first-run"):
            supplied = json.loads(before)["variants"][variant]
            loaded = c.load_criteria_bytes(before, variant=variant)
            self.assertEqual(loaded, supplied)
            self.assertNotIn("other", loaded)
        self.assertEqual(path.read_bytes(), before)
        with self.assertRaises(c.ContractError):
            c.load_criteria_bytes(before)

    def test_manifest_is_frozen_and_uses_immutable_bytes(self):
        manifest = self.manifest()
        with self.assertRaises(FrozenInstanceError):
            manifest.source_sha = "b" * 40
        changed = manifest.questions
        changed.clear()
        self.assertEqual(manifest.questions, self.questions)

    def test_manifest_records_original_byte_hashes(self):
        manifest = self.manifest()
        report = manifest.report([])
        self.assertEqual(report["criteria_bytes_sha256"], hashlib.sha256(self.criteria_raw).hexdigest())
        self.assertEqual(report["dataset_bytes_sha256"], c.sha256(b"synthetic mechanism bytes\n"))
        self.assertEqual(report["source_sha"], "a" * 40)
        self.assertFalse(report["immutable_server_model"])
        self.assertEqual(report["declared_questions"], self.questions)

    def test_manifest_refuses_modified_canonical_bytes(self):
        altered = c.encode(self.criteria)
        with self.assertRaisesRegex(c.ContractError, "canonical_pin"):
            c.Manifest("a" * 40, altered, b"synthetic", c.encode(self.questions), c.ENDPOINT, "fixture")

    def test_manifest_requires_explicit_source_sha(self):
        with self.assertRaisesRegex(c.ContractError, "source_sha"):
            c.Manifest("unknown", self.criteria_raw, b"synthetic", c.encode(self.questions), c.ENDPOINT, "fixture")

    def test_manifest_refuses_endpoint_substitution(self):
        with self.assertRaisesRegex(c.ContractError, "endpoint_policy"):
            c.Manifest("a" * 40, self.criteria_raw, b"synthetic", c.encode(self.questions),
                       "https://elsewhere.invalid", "fixture")

    def test_unknown_current_historical_and_synthetic_provenance_never_qualify(self):
        for sources in ([], [None], ["current-event: claimed"], ["dev-capture: claimed"],
                        ["synthetic mechanism fixture"]):
            status = c.qualification_status([{"source": source} for source in sources])
            self.assertEqual(status["runtime_parity"], "unproven")
            self.assertFalse(status["current_runtime_qualification"]["eligible"])
            self.assertFalse(status["automatic_actions_enabled"])

    def test_exact_declared_answer_set_positive(self):
        answer = c.validate_answers(self.answer(), self.questions)
        self.assertEqual(set(answer), set(self.questions))
        self.assertEqual(answer["label"]["choice"], "ready_idle")
        self.assertEqual(answer["label"]["confidence_source"], "choice_probability")

    def test_every_extra_supplied_question_must_be_answered(self):
        self.questions["supplied_event_question"] = {"type": "choice", "instructions": "opaque",
                                                     "criteria": {"yes": "opaque", "no": "opaque"}}
        payload = self.answer()
        with self.assertRaisesRegex(c.ContractError, "invalid_answer_set"):
            c.validate_answers(payload, self.questions)
        payload["answers"]["supplied_event_question"] = {
            "type": "choice", "choice": "yes", "probabilities": {"yes": 0.7}}
        self.assertEqual(set(c.validate_answers(payload, self.questions)), set(self.questions))

    def test_missing_and_undeclared_answers_refused(self):
        for name in ("label", "prompt_visible"):
            payload = self.answer()
            del payload["answers"][name]
            with self.assertRaisesRegex(c.ContractError, "invalid_answer_set"):
                c.validate_answers(payload, self.questions)
        payload = self.answer()
        payload["answers"]["undeclared"] = {"type": "noul", "noul": 0.5}
        with self.assertRaisesRegex(c.ContractError, "invalid_answer_set"):
            c.validate_answers(payload, self.questions)

    def test_answer_type_and_choice_refused(self):
        for field, value in (("type", "noul"), ("choice", "undeclared"), ("choice", [])):
            payload = self.answer()
            payload["answers"]["label"][field] = value
            with self.assertRaises(c.ContractError):
                c.validate_answers(payload, self.questions)

    def test_noul_requires_finite_probability(self):
        for value in (True, None, "0.1", float("inf"), float("nan"), -0.1, 1.1):
            payload = self.answer()
            payload["answers"]["prompt_visible"]["noul"] = value
            with self.assertRaisesRegex(c.ContractError, "invalid_noul"):
                c.validate_answers(payload, self.questions)

    def test_confidence_and_probabilities_require_finite_values(self):
        for value in (True, "0.9", float("nan"), -0.1, 1.1):
            payload = self.answer()
            payload["answers"]["label"]["confidence"] = value
            with self.assertRaisesRegex(c.ContractError, "invalid_confidence"):
                c.validate_answers(payload, self.questions)
            payload = self.answer()
            payload["answers"]["label"]["probabilities"] = {"ready_idle": value}
            with self.assertRaisesRegex(c.ContractError, "invalid_probabilities"):
                c.validate_answers(payload, self.questions)

    def test_question_schema_and_size_are_validated(self):
        for question in ({}, {"type": "unknown", "instructions": "opaque", "criteria": {"x": "opaque"}},
                         {"type": "noul", "instructions": "opaque", "criteria": {"x": "opaque"}}):
            with self.assertRaises(c.ContractError):
                c.validate_questions({"q": question})
        with self.assertRaisesRegex(c.ContractError, "questions_schema"):
            c.validate_questions({"q" + str(i): self.questions["label"] for i in range(33)})

    def test_json_rejects_nonfinite_duplicate_keys_and_oversize_before_parse(self):
        for raw, code in ((b'{"x":NaN}', "nonfinite_json"), (b'{"x":1,"x":2}', "duplicate_json_key"),
                          (b"x" * (c.MAX_RESPONSE + 1), "json_size")):
            with self.assertRaisesRegex(c.ContractError, code):
                c.decode(raw, c.MAX_RESPONSE)

    def test_wire_domains_are_distinct_and_do_not_claim_rust_hashes(self):
        raw = c.encode({"state": "opaque", "model": "fixture", "questions": self.questions})
        hashes = c.wire_hashes(raw)
        alternate = json.dumps(json.loads(raw), indent=2).encode()
        self.assertNotEqual(hashes["python_wire_bytes_sha256"], c.wire_hashes(alternate)["python_wire_bytes_sha256"])
        self.assertEqual(hashes["python_state_json_sha256"], c.sha256(c.encode("opaque")))
        self.assertEqual(hashes["python_question_json_sha256"], c.sha256(c.encode(self.questions)))

    def test_supplied_state_and_extra_questions_round_trip_without_defaults(self):
        self.questions["supplied_event_question"] = {"type": "noul", "instructions": "opaque",
                                                   "criteria": {"true": "opaque", "false": "opaque"}}
        supplied = {"opaque_context": "supplied", "seconds_since_spawn": None}
        raw = run.request_body("opaque", supplied, arm="context", model="fixture", criteria=self.criteria,
                               question_manifest=c.encode(self.questions))
        wire = c.validate_wire(raw)
        self.assertEqual(wire["state"], {"screen": "opaque", "context": supplied})
        self.assertEqual(wire["questions"], self.questions)
        self.assertEqual(set(wire["state"]["context"]), set(supplied))

    def test_request_size_and_nonfinite_state_refused(self):
        for state in ({"x": float("nan")}, {"x": "x" * c.MAX_REQUEST}):
            with self.assertRaises(c.ContractError):
                run.request_body("opaque", state, arm="context", model="fixture", criteria=self.criteria)

    def test_partial_or_incomplete_dataset_fails_before_transport(self):
        samples = [{"label": label, "transport": "pty" if i == 0 else "chat_backend"}
                   for label in run.LABELS for i in range(4)]
        with patch("run.JevTransport") as transport:
            with self.assertRaisesRegex(ValueError, "incomplete_dataset"):
                run.evaluate(samples, arm="screen", criteria=self.criteria, token="synthetic", endpoint=c.ENDPOINT,
                             model="fixture", timeout=60, complete_dataset=False)
        transport.assert_not_called()

    def test_historical_diagnostic_never_loads_token_or_infers(self):
        with patch("sys.argv", ["run.py", "--source-sha", "a" * 40,
                               "--criteria-mode", "historical-diagnostic"]), \
                patch("run.load_token") as token, patch("run.JevTransport") as transport, patch("builtins.print"):
            self.assertEqual(run.main(), 0)
        token.assert_not_called()
        transport.assert_not_called()


if __name__ == "__main__":
    unittest.main()
