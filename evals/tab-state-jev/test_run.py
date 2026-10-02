"""WHAT: classifier math, preflight and CLI consumers.
WHY: source preparation must preserve thresholds and refuse partial inference.
ORACLE: supplied mechanism values round-trip; no network admission on gaps.
Synthetic mechanism inputs only, never current event records.
"""

import unittest
import json
from unittest.mock import patch

import capture_screen
from prepare_fixtures import scrub_context
from run import (
    LABELS,
    aggregate_band_precision,
    aggregate_precision,
    band_for_confidence,
    coverage_gaps,
    call_jev,
    gate_verdict,
    request_body,
    evaluate,
    parse_args,
)


class CaptureScreenImportTests(unittest.TestCase):
    def test_capture_helper_imports(self):
        self.assertTrue(callable(capture_screen.parse_args))

    def test_provider_override_only_fills_missing_tab_identity(self):
        self.assertEqual(capture_screen.provider_id_for_tab({"agent_type": None}, "claude"), "claude")
        self.assertEqual(capture_screen.provider_id_for_tab({"agent_type": "codex"}, "claude"), "codex")
        with self.assertRaises(SystemExit):
            capture_screen.provider_id_for_tab({"agent_type": None}, None)

    def test_fixture_context_scrubs_tab_and_plan_ids(self):
        context = {
            "spawned_by_tab": "59981cea",
            "plan_id": "plan_secret",
            "task_id": None,
        }

        self.assertEqual(
            scrub_context(context),
            {"spawned_by_tab": "<id>", "plan_id": "<id>", "task_id": None},
        )


class BandMathTests(unittest.TestCase):
    def test_incomplete_coverage_rejects_inference(self):
        samples = [{"label": "ready_idle", "transport": "pty"}]
        with patch("run.JevTransport") as transport:
            with self.assertRaisesRegex(ValueError, "incomplete_dataset"):
                evaluate(samples, arm="screen", criteria={}, token="synthetic-token",
                         endpoint="https://api.typesafe.ai/v1/systemone", model="fixture", timeout=60)
        transport.assert_not_called()

    def test_cli_defaults_to_canonical_and_refuses_old_acceptance_override(self):
        args = parse_args(["--source-sha", "a" * 40])
        self.assertEqual(args.criteria_mode, "canonical")
        self.assertIsNone(args.criteria_file)
        with patch("sys.stderr"), self.assertRaises(SystemExit):
            parse_args(["--source-sha", "a" * 40, "--allow-incomplete"])

    def test_coverage_pools_label_support_across_providers(self):
        samples = [
            {
                "provider": provider,
                "label": "ready_idle",
                "transport": "pty",
                "provider_capability": {"display_name": "Claude"},
            }
            for provider in ("claude", "codex", "opencode", "cursor")
        ]

        gaps = coverage_gaps(samples)

        self.assertNotIn("ready_idle=4", gaps)
        self.assertIn("working_normally=0", gaps)
        self.assertIn("transport/chat_backend=0", gaps)

    def test_coverage_requires_both_transports(self):
        samples = [
            {"label": "ready_idle", "transport": "pty"},
            {"label": "ready_idle", "transport": "chat_backend"},
        ]

        gaps = coverage_gaps(samples)

        self.assertNotIn("transport/pty=0", gaps)
        self.assertNotIn("transport/chat_backend=0", gaps)

    def test_confidence_bands_include_threshold_edges(self):
        self.assertEqual(band_for_confidence(0.80), "strong")
        self.assertEqual(band_for_confidence(1.0), "strong")
        self.assertEqual(band_for_confidence(0.60), "weak")
        self.assertEqual(band_for_confidence(0.79), "weak")
        self.assertEqual(band_for_confidence(0.59), "ignored")

    def test_confidence_rejects_nonfinite_and_non_numeric_values(self):
        for value in (True, float("nan"), float("inf"), -0.1, 1.1, "0.8"):
            with self.subTest(value=value):
                with self.assertRaises(ValueError):
                    band_for_confidence(value)

    def test_aggregation_groups_by_provider_and_transport(self):
        samples = [
            {"id": "one", "label": "ready_idle", "provider": "claude", "transport": "pty"},
            {"id": "two", "label": "working_normally", "provider": "claude", "transport": "pty"},
            {"id": "three", "label": "ready_idle", "provider": "lmstudio", "transport": "chat_backend"},
        ]
        predictions = [
            {"id": "one", "label": "ready_idle", "prediction": "ready_idle"},
            {"id": "two", "label": "working_normally", "prediction": "ready_idle"},
            {"id": "three", "label": "ready_idle", "prediction": "ready_idle"},
        ]

        by_provider = aggregate_precision(samples, predictions, "provider")
        by_transport = aggregate_precision(samples, predictions, "transport")

        self.assertEqual(by_provider["claude"]["support"], 2)
        self.assertEqual(by_provider["claude"]["precision"], 0.5)
        self.assertEqual(by_provider["lmstudio"]["precision"], 1.0)
        self.assertEqual(by_transport["pty"]["calls"], 2)
        self.assertEqual(by_transport["pty"]["precision"], 0.5)
        self.assertEqual(by_transport["chat_backend"]["precision"], 1.0)

    def test_band_aggregation_keeps_provider_and_transport_groups(self):
        samples = [
            {"id": "one", "label": "ready_idle", "provider": "claude", "transport": "pty"},
            {"id": "two", "label": "working_normally", "provider": "claude", "transport": "pty"},
        ]
        predictions = [
            {"id": "one", "label": "ready_idle", "prediction": "ready_idle", "confidence": 0.9},
            {"id": "two", "label": "working_normally", "prediction": "ready_idle", "confidence": 0.65},
        ]

        by_provider = aggregate_band_precision(samples, predictions, "provider")
        by_transport = aggregate_band_precision(samples, predictions, "transport")

        self.assertEqual(by_provider["strong"]["claude"]["precision"], 1.0)
        self.assertEqual(by_provider["weak"]["claude"]["precision"], 0.0)
        self.assertEqual(by_transport["strong"]["pty"]["calls"], 1)
        self.assertEqual(by_transport["weak"]["pty"]["calls"], 1)

    def test_gate_verdict_requires_both_bands(self):
        passed = gate_verdict(0.90, 0.60)
        strong_failed = gate_verdict(0.89, 0.80)
        weak_failed = gate_verdict(0.95, 0.59)
        empty = gate_verdict(None, None)

        self.assertEqual(passed["verdict"], "pass")
        self.assertEqual(strong_failed["strong_actionable"]["verdict"], "fail")
        self.assertEqual(strong_failed["weak_overall"]["verdict"], "pass")
        self.assertEqual(weak_failed["weak_overall"]["verdict"], "fail")
        self.assertEqual(empty["verdict"], "fail")

    def test_request_arms_preserve_the_screen_and_context_shapes(self):
        criteria = {label: "opaque criterion" for label in LABELS}
        context = {"role": "Worker", "task_id": "task"}
        screen_only = json.loads(
            request_body("screen text", context, arm="screen", model="test", criteria=criteria)
        )
        contextual = json.loads(
            request_body("screen text", context, arm="context", model="test", criteria=criteria)
        )

        self.assertEqual(screen_only["state"], "screen text")
        self.assertEqual(contextual["state"], {"screen": "screen text", "context": context})
        self.assertEqual(set(screen_only["questions"]), {"q"})
        self.assertEqual(set(contextual["questions"]), {"label", "prompt_visible"})
        self.assertEqual(contextual["questions"]["label"]["criteria"], criteria)
        self.assertEqual(contextual["questions"]["prompt_visible"]["type"], "noul")

    def test_context_answers_require_both_named_answers(self):
        from jev_transport import TransportResult
        payload = {"answers": {
            "label": {"type": "choice", "choice": "ready_idle", "probabilities": {"ready_idle": 0.95}},
            "prompt_visible": {"type": "noul", "noul": 0.1},
        }}
        for missing in (False, True):
            with self.subTest(missing=missing):
                if missing:
                    del payload["answers"]["prompt_visible"]
                transport = unittest.mock.Mock()
                transport.send.return_value = TransportResult(payload, {"fixture_type": "synthetic_mechanism"})
                answer, _, error = call_jev(
                    "opaque screen", {}, arm="context", criteria={label: "opaque" for label in LABELS},
                    token="synthetic-token", endpoint="https://api.typesafe.ai/v1/systemone",
                    model="fixture", timeout=60, transport=transport,
                )
                if missing:
                    self.assertIsNone(answer)
                    self.assertEqual(error, "invalid_answer_set")
                else:
                    self.assertIsNone(error)
                    self.assertEqual(answer["prediction"], "ready_idle")
                    self.assertEqual(answer["prompt_visible"], 0.1)


if __name__ == "__main__":
    unittest.main()
