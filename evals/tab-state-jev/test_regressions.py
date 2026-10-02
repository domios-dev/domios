"""WHAT: independent canonical-map and bounded-response regression oracles.
WHY: the legacy loader rejects shipped bytes and the network reader is unbounded.
ORACLE: exact supplied map round-trip; small response succeeds, oversize fails.
Synthetic mechanism fixtures only; no event, credential or live-data evidence.
"""

import json
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

import jev_transport as transport_module

import run
import request_contract as contract
from jev_request_worker import read_bounded_response, WorkerError
from jev_transport import TransportFailure, TransportResult


class SyntheticReadTransport:
    def __init__(self, response):
        self.response = response

    def send(self, raw, **kwargs):
        try:
            body = read_bounded_response(self.response, end=60, clock=lambda: 0)
            return TransportResult(contract.decode(body, contract.MAX_RESPONSE),
                                   {"fixture_type": "synthetic_mechanism"})
        except WorkerError as error:
            raise TransportFailure(error.code) from error


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
                answer, _, error = run.call_jev(
                    "synthetic mechanism screen", {}, arm="screen",
                    criteria={label: "opaque" for label in run.LABELS},
                    token="synthetic-mechanism-token", endpoint=run.DEFAULT_ENDPOINT,
                    model="synthetic-alias", timeout=60, transport=SyntheticReadTransport(response),
                )
                self.assertEqual(error, expected_error)
                self.assertTrue(response.bounds)
                if expected_error is None:
                    self.assertEqual(answer["prediction"], "ready_idle")
                else:
                    self.assertIsNone(answer)


class WindowsPlatformTests(unittest.TestCase):
    def test_captured_windows_process_handle_is_closed_exactly_once(self):
        class CapturedHandle:
            def __int__(self):
                return 77
        handle = CapturedHandle()
        handle.Close = Mock()
        kernel = Mock()
        kernel.WaitForSingleObject.return_value = 0
        def exit_code(captured, pointer):
            pointer._obj.value = 0
            return 1
        kernel.GetExitCodeProcess.side_effect = exit_code
        with patch.object(transport_module.ctypes, "WinDLL", return_value=kernel, create=True):
            authority = transport_module.ProcessAuthority(
                SimpleNamespace(pid=999, returncode=None, _handle=handle), windows=True)
            self.assertEqual(authority.wait_exact(), 0)
            authority.close()
            authority.close()
            self.assertFalse(authority.cancel())
        handle.Close.assert_called_once()
        kernel.WaitForSingleObject.assert_called_once_with(77, 0xFFFFFFFF)
        kernel.TerminateProcess.assert_not_called()


if __name__ == "__main__":
    unittest.main()
