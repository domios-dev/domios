import tempfile
import unittest
from pathlib import Path
from evidence import keys, parse_jsonl, pointer, render, save_evidence, select_records


class EvidenceTests(unittest.TestCase):
    def test_matrix_unique_and_complete(self):
        cells = list(keys())
        self.assertEqual(len(cells), 408)
        self.assertEqual(len(set(cells)), len(cells))

    def test_no_receipt_never_passes(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            render(root, [])
            matrix = (root / 'results.md').read_text()
            self.assertEqual(matrix.count('| NOT RUN |'), 408)
            self.assertNotIn('| PASS |', matrix)

    def test_own_session_only(self):
        rows = parse_jsonl('{"sessionId":"own","value":1}\n{"sessionId":"foreign","value":2}')
        self.assertEqual(select_records(rows, '/sessionId', 'own'), [rows[0]])
        with self.assertRaises(ValueError):
            select_records(rows, '/sessionId', '')

    def test_malformed_record_is_not_silently_discarded(self):
        with self.assertRaises(ValueError):
            parse_jsonl('{broken')

    def test_pointer_escaping(self):
        self.assertEqual(pointer({'a/b': [{'~key': 7}]}, '/a~1b/0/~0key'), 7)

    def test_evidence_is_immutable_and_idempotent(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            first = save_evidence(root, b'failure evidence', '.txt')
            second = save_evidence(root, b'failure evidence', '.txt')
            self.assertEqual(first, second)
            self.assertEqual((root / first).read_bytes(), b'failure evidence')


if __name__ == '__main__':
    unittest.main()
