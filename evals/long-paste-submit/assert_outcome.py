import base64
import json
import pathlib
import sys


def assert_outcome(messages, ledger, frames, sends, backing="pty", before_ids=(), session=None):
    assert sends, "no sends to assert"
    assert backing in ("pty", "chat_backend"), f"unsupported backing: {backing}"
    for send_id, payload in sends:
        rows = [row for row in ledger if row.get("send_id") == send_id]
        assert len(rows) == 1 and rows[0].get("state") == "delivered", (
            f"send {send_id} must have one delivered ledger row: {rows}"
        )
        if session is not None:
            source = (rows[0].get("matched_row") or {}).get("source", {})
            assert source.get("source_session_id") == session, "missing exact own-session source receipt"
            record = source.get("record", {})
            if backing == "chat_backend":
                assert record.get("kind") == "structured" and record.get("thread_id") == session and record.get("item_id"), "missing runtime item receipt"
        own = [i for i, row in enumerate(messages)
               if row.get("type") == "user"
               and row.get("id") not in before_ids
               and not row.get("inheritedFromSessionId")
               and (row.get("content") or "").strip() == payload.strip()]
        assert len(own) == 1, f"send {send_id} exact own-session user turns={len(own)}, want 1"
        assert any(row.get("type") == "assistant" and row.get("content")
                   for row in messages[own[0] + 1:]), f"send {send_id} has no following assistant reply"
        if backing == "pty":
            raw = payload.encode("utf-8")
            matches = [frame for frame in frames if frame in (raw, b"\x1b[200~" + raw + b"\x1b[201~")]
            assert len(matches) == 1, f"send {send_id} captured payload writes={len(matches)}, want 1"


def main():
    case_dir = pathlib.Path(sys.argv[1])
    transcript = json.loads((case_dir / "assert-transcript.json").read_text())
    ledger = json.loads((case_dir / "assert-ledger.json").read_text())
    frames = []
    binding = json.loads((case_dir / "evidence-anchor.json").read_text())
    after = json.loads((case_dir / "evidence-binding-after.json").read_text())
    assert binding["session"] and binding["nonce"], "missing own-session authority"
    assert after == binding["binding"], "own-session binding changed during send"
    if binding["backing"] == "pty":
        for line in (case_dir / "trace.cast").read_text().splitlines()[1:]:
            row = json.loads(line)
            if row[1] == "i":
                frames.append(base64.b64decode(row[2]["bytes_b64"]))
    sends = [(sys.argv[i], pathlib.Path(sys.argv[i + 1]).read_text())
             for i in range(2, len(sys.argv), 2)]
    assert_outcome(transcript.get("messages", []), ledger, frames, sends,
                   binding["backing"], binding["before_ids"], binding["session"])
    print(f"PASS exact delivery and own-session reply: backing={binding['backing']} sends={len(sends)}")


if __name__ == "__main__":
    main()
