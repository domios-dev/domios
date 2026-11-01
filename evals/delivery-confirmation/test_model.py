"""Test confirmation evidence with isolated append-only fixtures.

Only consumed content can confirm a send; quoted assistant text and metadata
must fail while the original queued-command and glued-row oracle stays intact.
"""

import json
from pathlib import Path
import tempfile
import unittest

from model import matching_records, summarize


class EvidenceTests(unittest.TestCase):
    def test_real_consumed_glue_matches_complete_payload_after_anchor(self):
        fixture = Path(__file__).resolve().parents[2] / "plugins/provider-claude/tests/fixtures/owner-mid-turn-2.1.296.jsonl"
        records = [json.loads(line) for line in fixture.read_text().splitlines()]
        record = next(row for row in records if row.get("type") == "attachment")
        payload = record["attachment"]["prompt"].split("<domios")[0]
        session = record["sessionId"]
        with tempfile.TemporaryDirectory() as root:
            transcript = Path(root) / "own.jsonl"
            transcript.write_text(json.dumps(record) + "\n")
            anchor = transcript.stat().st_size
            foreign = {**record, "sessionId": "foreign"}
            truncated = {**record, "attachment": {**record["attachment"], "prompt": payload[:-1]}}
            with transcript.open("a") as stream:
                stream.write(json.dumps(foreign) + "\n" + json.dumps(truncated) + "\n")
            self.assertEqual(matching_records(transcript, anchor, session, payload), [])
            with transcript.open("a") as stream:
                stream.write(json.dumps(record) + "\n")
            matches = matching_records(transcript, anchor, session, payload)
            self.assertEqual(len(matches), 1)
            self.assertEqual(matches[0]["field"], "attachment.prompt")
            self.assertEqual(matches[0]["record_id"], record["uuid"])
            self.assertGreater(matches[0]["offset"], anchor)
            self.assertEqual(matches[0]["record"], record)

    def test_incomplete_append_is_not_a_committed_record(self):
        with tempfile.TemporaryDirectory() as root:
            transcript = Path(root) / "own.jsonl"
            transcript.write_text(json.dumps({"sessionId": "own", "type": "user",
                                              "message": {"role": "user", "content": "opaque"}}))
            self.assertEqual(matching_records(transcript, 0, "own", "opaque"), [])
            transcript.write_text(transcript.read_text() + "\n")
            self.assertEqual(len(matching_records(transcript, 0, "own", "opaque")), 1)

    def test_assistant_and_metadata_quotes_cannot_confirm(self):
        records = [
            {"type": "assistant", "message": {"role": "assistant", "content": "opaque"}},
            {"type": "user", "message": {"role": "user", "content": "different"}, "metadata": "opaque"},
            {"type": "user", "isMeta": True, "message": {"role": "user", "content": "opaque"}},
        ]
        with tempfile.TemporaryDirectory() as root:
            transcript = Path(root) / "own.jsonl"
            transcript.write_text("".join(json.dumps(row) + "\n" for row in records))
            self.assertEqual(matching_records(transcript, 0, "own", "opaque"), [])

    def test_tool_result_keeps_whole_payload_without_joining_partial_blocks(self):
        record = {"type": "user", "message": {"role": "user", "content": [
            {"type": "tool_result", "content": [
                {"type": "text", "text": "prefix\nopaque\nwhole\nsuffix"},
                {"type": "text", "text": "partial"}, {"type": "text", "text": "other"}]}]}}
        with tempfile.TemporaryDirectory() as root:
            transcript = Path(root) / "own.jsonl"
            transcript.write_text(json.dumps(record) + "\n")
            self.assertEqual(len(matching_records(transcript, 0, "own", "opaque\nwhole")), 1)
            self.assertEqual(matching_records(transcript, 0, "own", "partial\nother"), [])

    def test_structured_user_content_can_confirm_without_provider_identity_branch(self):
        record = {"type": "response_item", "payload": {"type": "message", "role": "user",
                  "content": [{"type": "input_text", "text": "opaque"}]}}
        with tempfile.TemporaryDirectory() as root:
            transcript = Path(root) / "own.jsonl"
            transcript.write_text(json.dumps(record) + "\n")
            self.assertEqual(len(matching_records(transcript, 0, "own", "opaque")), 1)

    def test_duplicate_consumption_is_not_counted_as_confirmation(self):
        group = summarize([{"provider": "fixture", "path": "cli", "mode": "idle",
                            "state": "delivered", "evidence": [{}, {}]}])[0]
        self.assertEqual(group["confirmed"], 0)
        self.assertEqual(group["confirmation_rate"], 0)

    def test_rates_require_delivered_ledger_and_exact_record_per_provider_path_mode(self):
        rows = []
        for provider in ("claude", "codex"):
            for path in ("composer", "cli"):
                for mode in ("idle", "busy"):
                    for index in range(10):
                        rows.append({"provider": provider, "path": path, "mode": mode,
                                     "state": "delivered" if index < 8 else "submit_unconfirmed",
                                     "evidence": [{}] if index != 0 else []})
        groups = summarize(rows)
        self.assertEqual(len(groups), 8)
        for group in groups:
            self.assertEqual(group["total"], 10)
            self.assertEqual(group["confirmed"], 7)
            self.assertEqual(group["received_unconfirmed"], 2)
            self.assertEqual(group["confirmation_rate"], 0.7)


if __name__ == "__main__":
    unittest.main()
