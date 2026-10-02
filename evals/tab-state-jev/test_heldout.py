import unittest
from heldout import AUTOMATIC_LABELS, NEGATIVES, assert_separate, qualification


class HeldoutTests(unittest.TestCase):
    def frames(self):
        return [{"id": f"{label}-{i}", "label": label, "screen": f"{label}-{i}"}
                for label in AUTOMATIC_LABELS + NEGATIVES for i in range(4)]

    def test_precision_band_boundaries(self):
        samples = self.frames()
        predictions = [dict(row, prediction=row["label"], confidence=0.90) for row in samples]
        result = qualification(samples, predictions)
        self.assertEqual(result["decision"], "qualifies_for_live_oracle")
        self.assertEqual(result["automatic_band"]["predicted"], 16)
        self.assertFalse(result["automatic_actions_enabled"])
        predictions[0]["confidence"] = 0.89
        self.assertEqual(qualification(samples, predictions)["decision"], "notify_only")

    def test_incomplete_slice_and_errors_cannot_qualify(self):
        samples = self.frames()
        predictions = [dict(row, prediction=row["label"], confidence=0.99) for row in samples]
        self.assertEqual(qualification(samples, predictions, complete_dataset=False)["decision"], "notify_only")
        predictions[0] = {"id": "failure"}
        self.assertEqual(qualification(samples, predictions)["decision"], "notify_only")

    def test_overlap_and_duplicate_screens_are_rejected(self):
        frame = {"id": "new", "screen": "same screen"}
        with self.assertRaises(ValueError):
            assert_separate([frame], [{"id": "old", "screen": "same screen"}])
        with self.assertRaises(ValueError):
            assert_separate([frame, dict(frame, id="different")], [])

    def test_notification_band_includes_point_eight(self):
        from heldout import band_metrics, NOTIFICATION_LABELS, OWNER_NOTIFICATION_THRESHOLD
        rows = [{"prediction": "login_or_auth_required", "label": "login_or_auth_required", "confidence": confidence}
                for confidence in [0.79, 0.80, 0.89, 0.90]]
        self.assertEqual(band_metrics(rows, NOTIFICATION_LABELS, OWNER_NOTIFICATION_THRESHOLD)["predicted"], 3)
