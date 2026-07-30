#!/usr/bin/env python3
import io
import importlib.util
from pathlib import Path
import unittest
from contextlib import redirect_stdout
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("rt_lib", Path(__file__).with_name("rt_lib.py"))
rt_lib = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rt_lib)


def order_check(messages):
    stdin = io.StringIO('{"messages":' + messages + "}")
    stdout = io.StringIO()
    with patch("sys.stdin", stdin), redirect_stdout(stdout):
        rt_lib.cmd_order_check("TEST", "5")
    return stdout.getvalue().strip()


class OrderCheckTests(unittest.TestCase):
    def test_boundary_collapse_reports_indices_position_and_sequence(self):
        result = order_check(
            '[{"type":"user","content":"msg 1 of TEST msg 2 of TEST"},'
            '{"type":"user","content":"msg 3 of TEST"},'
            '{"type":"user","content":"msg 4 of TEST"},'
            '{"type":"user","content":"msg 5 of TEST"}]'
        )
        self.assertEqual(result, "collapse 1,2@0; positions 0,0,1,2,3")

    def test_reorder_reports_sequence(self):
        result = order_check(
            '[{"type":"user","content":"msg 2 of TEST"},'
            '{"type":"user","content":"msg 1 of TEST"},'
            '{"type":"user","content":"msg 3 of TEST"},'
            '{"type":"user","content":"msg 4 of TEST"},'
            '{"type":"user","content":"msg 5 of TEST"}]'
        )
        self.assertEqual(result, "reorder 1,0,2,3,4")

    def test_missing_label_is_unchanged(self):
        result = order_check(
            '[{"type":"user","content":"msg 1 of TEST"},'
            '{"type":"user","content":"msg 2 of TEST"}]'
        )
        self.assertEqual(result, "missing 3")


if __name__ == "__main__":
    unittest.main()
