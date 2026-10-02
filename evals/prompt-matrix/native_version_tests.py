"""Dedicated native-version resource controls; all execution dependencies are fake."""

import unittest

from native_assembly import qualify_receipt
from native_layout import FixtureError


class NativeVersionControls(unittest.TestCase):
    def test_legacy_receipt_is_refused(self):
        receipt = {"status": "verified", "version": "supplied-version", "executable_sha256": "supplied-sha",
                   "exit_code": 0, "argv": ["supplied-executable", "--version"]}
        with self.assertRaisesRegex(FixtureError, "unqualified"):
            qualify_receipt(receipt, "supplied-version", "supplied-executable", "supplied-sha")


if __name__ == "__main__":
    unittest.main()
