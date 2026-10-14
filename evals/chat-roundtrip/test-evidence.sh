#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
OUT_DIR="$(mktemp -d)"
PY="$(command -v python3)"
trap 'rm -rf "$OUT_DIR"' EXIT
source "$ROOT/evals/chat-roundtrip/evidence.sh"

dev_get() {
  printf '%s\n' "$1" >> "$OUT_DIR/reads.txt"
  "$PY" -c 'import json,sys; print(json.dumps({"route":sys.argv[1]}))' "$1"
}
dev_post() {
  "$PY" -c 'import json,sys; print(json.dumps({"route":sys.argv[1],"body":json.loads(sys.argv[2])}))' "$1" "$2" >> "$OUT_DIR/writes.jsonl"
  printf '{"ok":true}\n'
}

FAILS=7
BENCH_ASSERTIONS=11
picker_probe_trigger claude tab123 > "$OUT_DIR/stdout.txt"
capture_chat_roundtrip_evidence claude tab123 picker-result
payload='opaque `body` $value
next "line"'
response='{"send_id":"send123","state":"written"}'
capture_detached_send_evidence claude tab123 detached-admitted send123 "$payload" "$response"
capture_detached_send_evidence claude tab123 detached-final send123 "$payload" "$response"

"$PY" - "$OUT_DIR" "$payload" "$FAILS" "$BENCH_ASSERTIONS" <<'PY'
import json
from pathlib import Path
import sys

root = Path(sys.argv[1])
directory = root / "evidence/claude-tab123"
writes = [json.loads(line) for line in (root / "writes.jsonl").read_text().splitlines()]
assert writes == [
    {"route": "/api/tabs/tab123/send", "body": {"text": "/model", "raw": True}},
    {"route": "/api/tabs/tab123/send", "body": {"text": "\r", "raw": True}},
]
for index, name in enumerate(["command", "submit"]):
    assert json.loads((directory / f"picker-{name}-request.json").read_text()) == writes[index]["body"]
    assert json.loads((directory / f"picker-{name}-response.json").read_text()) == {"ok": True}
for stage in ["picker-before", "picker-after-trigger", "picker-result", "detached-admitted", "detached-final"]:
    for suffix, route in [("screen", "/api/tabs/tab123/text"), ("formatted-screen", "/api/tabs/tab123/text?formatted=true"), ("mux", "/api/mux/state"), ("status", "/api/tabs/tab123/status")]:
        assert json.loads((directory / f"{stage}-{suffix}.json").read_text()) == {"route": route}
for stage in ["detached-admitted", "detached-final"]:
    send = json.loads((directory / f"{stage}-send.json").read_text())
    assert send == {"provider": "claude", "tab_id": "tab123", "stage": stage, "send_id": "send123", "payload": sys.argv[2]}
    assert json.loads((directory / f"{stage}-response.json").read_text()) == {"send_id": "send123", "state": "written"}
    assert json.loads((directory / f"{stage}-own-transcript.json").read_text()) == {"route": "/api/tabs/tab123/chat?since=0"}
reads = (root / "reads.txt").read_text().splitlines()
assert reads.count("/api/tabs/tab123/chat-send") == 1
assert not (directory / "detached-admitted-ledger-after-oracle.json").exists()
assert json.loads((directory / "detached-final-ledger-after-oracle.json").read_text()) == {"route": "/api/tabs/tab123/chat-send"}
assert (root / "stdout.txt").read_bytes() == b""
assert sys.argv[3:] == ["7", "11"]
print("36 evidence retention assertions passed")
PY

dev_post() { return 9; }
if picker_probe_trigger codex tab456; then
  echo 'failed raw request was concealed' >&2
  exit 1
fi
if capture_chat_roundtrip_evidence claude tab123 '../unsafe'; then
  echo 'unsafe artifact path was accepted' >&2
  exit 1
fi
echo '2 evidence failure controls passed'
