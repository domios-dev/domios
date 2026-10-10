import json
from pathlib import Path
import tempfile
import unittest
from evidence import PROVIDERS, coordinate, matrix, record, render, validate


class EvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / 'observations').mkdir()
        (self.root / 'manifest.json').write_text(json.dumps({'providers': PROVIDERS}))
        self.row = dict(matrix()[0], status='PASS', tab_id='fixture-tab', observation='synthetic fixture', attempt='one', evidence={})
        for key in ('home_before', 'home_after', 'view_before', 'view_after', 'home_formatted', 'view_formatted', 'renderer_state', 'event_evidence'):
            (self.root / key).write_text('fixture')
            self.row['evidence'][key] = key

    def test_full_matrix_and_optional_provider(self):
        self.assertEqual(len(matrix()), 240)
        self.assertEqual(len({coordinate(r) for r in matrix()}), 240)
        self.assertEqual(len(matrix(PROVIDERS + ('mimo',))), 288)

    def test_missing_renderer_evidence_rejected(self):
        del self.row['evidence']['view_after']
        with self.assertRaises(KeyError):
            validate(self.root, self.row)

    def test_empty_evidence_rejected(self):
        (self.root / 'view_after').write_text('')
        with self.assertRaises(ValueError):
            validate(self.root, self.row)

    def test_path_escape_rejected(self):
        self.row['evidence']['home_before'] = '../foreign'
        with self.assertRaises(ValueError):
            validate(self.root, self.row)

    def test_agent_requires_session_record(self):
        self.row['provider'] = 'codex'
        with self.assertRaises(ValueError):
            validate(self.root, self.row)

    def test_observation_never_overwritten(self):
        record(self.root, self.row)
        with self.assertRaises(FileExistsError):
            record(self.root, self.row)

    def test_fail_survives_later_pass(self):
        self.row.update(status='FAIL')
        for key in ('repro', 'logs'):
            (self.root / key).write_text('synthetic failure detail')
            self.row['evidence'][key] = key
        record(self.root, self.row)
        self.row.update(status='PASS', attempt='two')
        record(self.root, self.row)
        self.assertIn('| shell | output | local | mac-home | FAIL |', (self.root / 'results.md').read_text())
        self.assertTrue((self.root / 'observations/one.json').exists())

    def test_unexecuted_rows_stay_pending(self):
        render(self.root, PROVIDERS)
        self.assertEqual((self.root / 'results.md').read_text().count('| PENDING |'), 240)


if __name__ == '__main__':
    unittest.main()
