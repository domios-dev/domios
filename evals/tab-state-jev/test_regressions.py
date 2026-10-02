"""WHAT: independent canonical-map and bounded-response regression oracles.
WHY: the legacy loader rejects shipped bytes and the network reader is unbounded.
ORACLE: exact supplied map round-trip; small response succeeds, oversize fails.
Synthetic mechanism fixtures only; no event, credential or live-data evidence.
"""

import json
from pathlib import Path
import unittest
from unittest.mock import patch

import run


class BoundedResponse:
    def __init__(self, raw):
        self.raw = raw
        self.offset = 0
        self.bounds = []

    def __enter__(self):
        return self

    def __exit__(self, *args):
        return False

    def read(self, size=None):
        if not isinstance(size, int) or size <= 0:
            raise AssertionError("response read requires an explicit positive byte bound")
        self.bounds.append(size)
        chunk = self.raw[self.offset:self.offset + size]
        self.offset += len(chunk)
        return chunk


def valid_response():
    return json.dumps({"answers": {"q": {
        "type": "choice", "choice": "ready_idle",
        "probabilities": {"ready_idle": 0.95},
    }}}).encode()


class LegacyRegressionTests(unittest.TestCase):
    def test_canonical_criteria_plain_map_is_loadable(self):
        path = Path(run.__file__).with_name("criteria.json")
        supplied = path.read_bytes()
        self.assertEqual(run.load_criteria(path, "current"), json.loads(supplied))

    def test_response_reads_are_explicitly_byte_bounded(self):
        small = BoundedResponse(valid_response())
        oversized = BoundedResponse(valid_response() + b" " * (64 * 1024 + 1))
        for response, expected_error in ((small, None), (oversized, "response_oversize")):
            with self.subTest(oversized=expected_error is not None):
                with patch("run.urllib.request.urlopen", return_value=response):
                    answer, _, error = run.call_jev(
                        "synthetic mechanism screen", {}, arm="screen",
                        criteria={label: "opaque" for label in run.LABELS},
                        token="synthetic-mechanism-token", endpoint=run.DEFAULT_ENDPOINT,
                        model="synthetic-alias", timeout=60,
                    )
                self.assertEqual(error, expected_error)
                self.assertTrue(response.bounds)
                if expected_error is None:
                    self.assertEqual(answer["prediction"], "ready_idle")
                else:
                    self.assertIsNone(answer)


if __name__ == "__main__":
    unittest.main()
