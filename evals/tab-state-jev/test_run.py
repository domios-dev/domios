import unittest
import json
import tempfile
from pathlib import Path

import capture_screen
from prepare_fixtures import scrub_context
from run import (
    LABELS,
    aggregate_band_precision,
    aggregate_precision,
    band_for_confidence,
    coverage_gaps,
    gate_verdict,
    read_dataset,
    request_body,
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
    def test_incomplete_coverage_is_opt_in_for_exploratory_runs(self):
        capability = {"transport": "pty", "display_name": "Claude"}
        row = {
            "id": "sample",
            "provider": "claude",
            "transport": "pty",
            "provider_capability": capability,
            "label": "ready_idle",
            "labelled_by": "watchman",
            "source": "dev-capture: test fixture",
            "screen": "ready",
            "context": {
                "provider_capability": capability,
                "requested_model": None,
                "session_model": None,
                "role": None,
                "plan_id": None,
                "task_id": None,
                "spawned_by_tab": "<id>",
                "seconds_since_spawn": 1,
                "seconds_since_last_output": 0,
                "structural_verdict": None,
                "pending_prompt_kind": None,
                "screen_changed": False,
            },
        }

        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "screens.jsonl"
            path.write_text(json.dumps(row) + "\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "at least 4 real frames per label"):
                read_dataset(path)
            self.assertEqual(len(read_dataset(path, require_coverage=False)), 1)

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


if __name__ == "__main__":
    unittest.main()
