"""Test the transport-specific delivery oracle on isolated small fixtures.

Runtime delivery needs an exact own-thread receipt and fresh payload row; PTY
delivery additionally needs one payload write. No process or network is used.
"""

import unittest

from assert_outcome import assert_outcome
from observe import binding


class OutcomeTests(unittest.TestCase):
    def setUp(self):
        self.messages = [{"id": "fresh", "type": "user", "content": "opaque"},
                         {"id": "reply", "type": "assistant", "content": "result"}]
        self.row = {"send_id": "send", "state": "delivered", "matched_row": {
            "source": {"source_session_id": "own", "record": {
                "kind": "structured", "thread_id": "own", "item_id": "fresh"}}}}

    def check(self, **overrides):
        args = dict(messages=self.messages, ledger=[self.row], frames=[],
                    sends=[("send", "opaque")], backing="chat_backend", session="own")
        args.update(overrides)
        assert_outcome(**args)

    def test_runtime_needs_no_pty_capture(self):
        self.check()

    def test_pty_requires_one_payload_write(self):
        with self.assertRaisesRegex(AssertionError, "payload writes=0"):
            self.check(backing="pty")
        self.check(backing="pty", frames=[b"\x1b[200~opaque\x1b[201~"])

    def test_second_payload_write_fails(self):
        with self.assertRaisesRegex(AssertionError, "payload writes=2"):
            self.check(backing="pty", frames=[b"opaque", b"opaque"])

    def test_unconfirmed_ledger_never_passes(self):
        with self.assertRaisesRegex(AssertionError, "delivered ledger"):
            self.check(ledger=[{**self.row, "state": "submit_unconfirmed"}])

    def test_duplicate_payload_turn_fails(self):
        with self.assertRaisesRegex(AssertionError, "user turns=2"):
            self.check(messages=[self.messages[0], *self.messages])

    def test_pre_anchor_turn_fails(self):
        with self.assertRaisesRegex(AssertionError, "user turns=0"):
            self.check(before_ids=["fresh"])

    def test_inherited_payload_fails(self):
        with self.assertRaisesRegex(AssertionError, "user turns=0"):
            self.check(messages=[{**self.messages[0], "inheritedFromSessionId": "old"}, self.messages[1]])

    def test_truncated_payload_fails(self):
        with self.assertRaisesRegex(AssertionError, "user turns=0"):
            self.check(sends=[("send", "opaque-full")])

    def test_foreign_or_missing_source_receipt_fails(self):
        with self.assertRaisesRegex(AssertionError, "own-session source"):
            self.check(session="foreign")
        with self.assertRaisesRegex(AssertionError, "own-session source"):
            self.check(ledger=[{"send_id": "send", "state": "delivered"}])

    def test_wrong_runtime_thread_fails(self):
        self.row["matched_row"]["source"]["record"]["thread_id"] = "foreign"
        with self.assertRaisesRegex(AssertionError, "runtime item receipt"):
            self.check()

    def test_absent_following_reply_fails(self):
        with self.assertRaisesRegex(AssertionError, "following assistant"):
            self.check(messages=self.messages[:1])

    def test_binding_requires_acknowledged_nonce(self):
        tab = {"agent_session_id": "own", "session_binding_nonce": "nonce",
               "reported_session_binding_nonce": "nonce",
               "backing": {"kind": "chat_backend", "session_id": "own"}}
        self.assertEqual(binding(tab)["session"], "own")
        tab["reported_session_binding_nonce"] = "foreign"
        with self.assertRaisesRegex(ValueError, "acknowledged"):
            binding(tab)

    def test_backing_thread_must_agree_with_claimed_session(self):
        tab = {"agent_session_id": "own", "session_binding_nonce": "nonce",
               "reported_session_binding_nonce": "nonce",
               "backing": {"kind": "chat_backend", "session_id": "foreign"}}
        with self.assertRaisesRegex(ValueError, "disagrees"):
            binding(tab)


if __name__ == "__main__":
    unittest.main()
