import unittest
import json

from run import LABELS, aggregate_precision, band_for_confidence, gate_verdict, request_body


class BandMathTests(unittest.TestCase):
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
