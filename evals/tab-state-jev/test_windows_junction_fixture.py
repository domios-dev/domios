from contextlib import contextmanager
import ctypes
import struct
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

from jev_request_worker import WorkerError
import windows_junction_fixture as junction


class JunctionFixtureTests(unittest.TestCase):
    def fixture(self, *, set_error=False, read_error=False, wrong_identity=False):
        raw = junction.mount_point_buffer(r"C:\owned\target")
        events = []

        def control(handle, code, source, size, output, capacity, returned, overlapped):
            events.append(code)
            self.assertEqual(handle, 77)
            self.assertIsNone(overlapped)
            if code == junction.SET_REPARSE:
                self.assertEqual(source.raw[:size], raw)
                return not set_error
            ctypes.memmove(output, raw, len(raw))
            returned._obj.value = len(raw)
            return not read_error

        @contextmanager
        def retiring(owner):
            try:
                yield
            finally:
                events.append("closed")

        before = {"directory": True, "final": r"C:\owned\control", "identity": b"supplied identity"}
        after = dict(before, attributes=0x400, identity=b"wrong" if wrong_identity else before["identity"])
        api = SimpleNamespace(kernel=SimpleNamespace(CreateFileW=Mock(return_value=77), DeviceIoControl=Mock(side_effect=control)),
                              inspect=Mock(return_value=before), identify=Mock(return_value=after),
                              create_directory=Mock(), captured=Mock(), retiring=retiring)
        guard = SimpleNamespace(api=api, check_time=Mock(), revalidate=Mock())
        return guard, events

    def test_mount_point_buffer_has_exact_tag_offsets_and_terminated_names(self):
        raw = junction.mount_point_buffer(r"C:\owned\target")
        tag, size, reserved, offset, length, print_offset, print_length = struct.unpack_from("<IHHHHHH", raw)
        self.assertEqual((tag, size, reserved, offset), (0xA0000003, len(raw) - 8, 0, 0))
        self.assertEqual(raw[16:16 + length].decode("utf-16-le"), r"\??\C:\owned\target")
        self.assertEqual(print_offset, length + 2)
        self.assertEqual(raw[16 + print_offset:16 + print_offset + print_length].decode("utf-16-le"), r"C:\owned\target")
        self.assertEqual(raw[-2:], b"\0\0")

    def test_native_set_readback_identity_and_owned_handle_retirement(self):
        guard, events = self.fixture()
        result = junction.create_junction(guard, r"C:\owned\control", r"C:\owned\target", "opaque descriptor")
        self.assertEqual(result, {"kind": "directory_junction", "tag": 0xA0000003, "identity": b"supplied identity".hex()})
        self.assertEqual(events, [junction.SET_REPARSE, junction.GET_REPARSE, "closed"])
        guard.api.captured.assert_called_once_with("file", 77)
        self.assertEqual(guard.api.kernel.CreateFileW.call_args.args[5] & 0x00200000, 0x00200000)

    def test_native_failures_preserve_numeric_cause_and_retire_handle(self):
        for options in ({"set_error": True}, {"read_error": True}, {"wrong_identity": True}):
            with self.subTest(options=options), patch.object(ctypes, "get_last_error", return_value=5, create=True):
                guard, events = self.fixture(**options)
                with self.assertRaises(WorkerError) as failure:
                    junction.create_junction(guard, r"C:\owned\control", r"C:\owned\target", "opaque descriptor")
                self.assertEqual(events[-1], "closed")
                if not options.get("wrong_identity"):
                    self.assertEqual(failure.exception.facts, {"reparse_creation_os": {"winerror": 5}})


if __name__ == "__main__":
    unittest.main()
