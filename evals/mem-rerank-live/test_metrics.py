import unittest
import sys
from hashlib import sha256
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from import_titles import disambiguate_title, proposed_item_id, title_slug
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


class ImportTitleTests(unittest.TestCase):
    def test_existing_dev_seed_id_is_avoided(self):
        title = "Earlier dev seed"
        seed_id = proposed_item_id("fact", title)
        imported_title = disambiguate_title(
            title,
            "fact_source_new",
            "fact",
            {seed_id},
        )

        self.assertNotEqual(proposed_item_id("fact", imported_title), seed_id)
        self.assertIn("[src:", imported_title)

    def test_duplicate_title_gets_stable_unique_dev_id(self):
        title = "Repeated memory title"
        first_id = proposed_item_id("fact", title)
        second_title = disambiguate_title(
            title,
            "fact_source_second",
            "fact",
            {first_id},
            force_suffix=True,
        )
        second_id = proposed_item_id("fact", second_title)
        repeated_title = disambiguate_title(
            title,
            "fact_source_second",
            "fact",
            {first_id, second_id},
            force_suffix=True,
            own_id=second_id,
        )

        self.assertNotEqual(first_id, second_id)
        self.assertEqual(second_title, repeated_title)
        self.assertIn("[src:", second_title)

    def test_suffix_survives_max_slug_length(self):
        title = "long title " * 20
        result = disambiguate_title(
            title,
            "fact_source_long_title",
            "fact",
            set(),
            force_suffix=True,
        )

        self.assertLessEqual(len(title_slug(result)), 60)
        self.assertTrue(
            title_slug(result).endswith(
                "src_" + sha256(b"fact_source_long_title").hexdigest()[:8]
            )
        )

    def test_suffix_stays_within_mem_title_limit(self):
        title = "é" * 100
        result = disambiguate_title(
            title,
            "fact_source_unicode_title",
            "fact",
            set(),
            force_suffix=True,
        )

        self.assertLessEqual(len(result.encode("utf-8")), 200)


if __name__ == "__main__":
    unittest.main()
