import unittest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from metrics import first_relevant_rank, gate_counts, reciprocal_rank


class RankingMetricTests(unittest.TestCase):
    def test_rank_uses_ground_truth_ids(self):
        self.assertEqual(first_relevant_rank(["x", "answer", "y"], ["answer"]), 2)
        self.assertIsNone(first_relevant_rank(["x", "y"], ["missing"]))

    def test_mrr_reciprocal_rank_cutoff(self):
        self.assertEqual(reciprocal_rank(2), 0.5)
        self.assertEqual(reciprocal_rank(11), 0.0)
        self.assertEqual(reciprocal_rank(None), 0.0)

    def test_gate_precision_and_recall(self):
        result = gate_counts([0.05, 0.20, 0.04, None], [True, True, False, False], 0.10)
        self.assertEqual(result["true_positive"], 1)
        self.assertEqual(result["false_positive"], 1)
        self.assertEqual(result["false_negative"], 1)
        self.assertEqual(result["precision"], 0.5)
        self.assertEqual(result["recall"], 0.5)


if __name__ == "__main__":
    unittest.main()
