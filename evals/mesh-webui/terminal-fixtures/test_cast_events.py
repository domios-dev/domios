import json
import unittest
from cast_events import parse_cast


class CastTests(unittest.TestCase):
    def fixture(self, events):
        return '\n'.join(json.dumps(v) for v in [{'version': 2, 'width': 80, 'height': 24}, *events])

    def test_ansi_unicode_and_duplicate_events_preserved(self):
        value = '\x1b[31m界λ\x1b[0m'
        _, events = parse_cast(self.fixture([[0, 'o', value], [1, 'o', value]]))
        self.assertEqual([e['data'] for e in events], [value, value])

    def test_resize_preserved(self):
        _, events = parse_cast(self.fixture([[1, 'r', '60x20']]))
        self.assertEqual(events[0], {'at': 1, 'kind': 'r', 'data': '60x20'})

    def test_backward_timestamp_refused(self):
        with self.assertRaises(ValueError):
            parse_cast(self.fixture([[2, 'o', 'a'], [1, 'o', 'b']]))

    def test_malformed_event_refused(self):
        with self.assertRaises(ValueError):
            parse_cast(self.fixture([[1, 'o']]))


if __name__ == '__main__':
    unittest.main()
