import importlib.util
import json
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("embedding_parity", ROOT / "scripts/embedding-parity.py")
ORACLE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ORACLE)


class EmbeddingParityTests(unittest.TestCase):
    def test_rejects_a_different_direction(self):
        old = [[1.0] + [0.0] * 383]
        new = [[0.0, 1.0] + [0.0] * 382]
        with self.assertRaisesRegex(ValueError, "parity failed"):
            ORACLE.compare_vectors(old, new, 1)

    def test_rejects_dimensions_counts_and_unnormalized_vectors(self):
        valid = [[1.0] + [0.0] * 383]
        for invalid in [[], [[1.0]], [[2.0] + [0.0] * 383], [[float("nan")] * 384]]:
            with self.subTest(invalid=invalid):
                with self.assertRaises(ValueError):
                    ORACLE.compare_vectors(valid, invalid, 1)

    def test_golden_matches_the_fixed_inputs_and_has_unit_vectors(self):
        fixture = json.loads((ROOT / "src-tauri/crates/codeterm-mem/tests/fixtures/embed_parity.json").read_text(encoding="utf-8"))
        inputs = json.loads((ROOT / "scripts/fixtures/embedding-parity.json").read_text(encoding="utf-8"))
        self.assertEqual([case["text"] for case in fixture["cases"]], inputs)
        vectors = [case["vector"] for case in fixture["cases"]]
        self.assertEqual(len(ORACLE.compare_vectors(vectors, vectors, 25)), 25)


if __name__ == "__main__":
    unittest.main()
