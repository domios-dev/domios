import base64
import json
import pathlib
import sys


def assert_outcome(messages, ledger, frames, sends):
    assert sends, "no sends to assert"
    for send_id, payload in sends:
        rows = [row for row in ledger if row.get("send_id") == send_id]
        assert len(rows) == 1 and rows[0].get("state") == "delivered", (
            f"send {send_id} must have one delivered ledger row: {rows}"
        )
        own = [i for i, row in enumerate(messages)
               if row.get("type") == "user"
               and (row.get("content") or "").strip() == payload.strip()]
        assert len(own) == 1, f"send {send_id} exact own-session user turns={len(own)}, want 1"
        assert any(row.get("type") == "assistant" and row.get("content")
                   for row in messages[own[0] + 1:]), f"send {send_id} has no following assistant reply"
        raw = payload.encode("utf-8")
        matches = [frame for frame in frames if frame in (raw, b"\x1b[200~" + raw + b"\x1b[201~")]
        assert len(matches) == 1, f"send {send_id} captured payload writes={len(matches)}, want 1"


def main():
    case_dir = pathlib.Path(sys.argv[1])
    transcript = json.loads((case_dir / "assert-transcript.json").read_text())
    ledger = json.loads((case_dir / "assert-ledger.json").read_text())
    frames = []
    for line in (case_dir / "trace.cast").read_text().splitlines()[1:]:
        row = json.loads(line)
        if row[1] == "i":
            frames.append(base64.b64decode(row[2]["bytes_b64"]))
    sends = [(sys.argv[i], pathlib.Path(sys.argv[i + 1]).read_text())
             for i in range(2, len(sys.argv), 2)]
    assert_outcome(transcript.get("messages", []), ledger, frames, sends)
    print(f"PASS exact delivery, own-session reply and single payload write: sends={len(sends)}")


if __name__ == "__main__":
    main()
