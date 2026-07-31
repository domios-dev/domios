#!/usr/bin/env bash
# Streaming-preview recording driver (dev :7686 only).
# Usage: drive-recording.sh <provider-tab-id> <provider-name>
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$REPO_ROOT/evals/_lib/lib.sh"

TAB="${1:?usage: drive-recording.sh <tab-id> <label> [questions.json] }"
PROVIDER="${2:?usage: drive-recording.sh <tab-id> <label> [questions.json] }"
QUESTIONS="${3:-$REPO_ROOT/testbench/streaming/questions.json}"
OUT_DIR="$REPO_ROOT/captures/streaming/$PROVIDER"
CAST_PATH="$OUT_DIR/session.cast"
mkdir -p "$OUT_DIR"
CAPTURE_ACTIVE=0
SIDECAR_PID=""

wait_status() {
  python3 - "$TAB" "$OUT_DIR/questions.log" "$1" "${2:-300}" <<'PYEOF'
import json
import os
import socket
import sys
import time
import urllib.error
import urllib.request

tab, question_log, mode, timeout_s = sys.argv[1], sys.argv[2], sys.argv[3], float(sys.argv[4])
POLL_INTERVAL_S = 0.25
token = os.environ["CODETERM_API_TOKEN"]
headers = {"Authorization": f"Bearer {token}"}
base = "http://127.0.0.1:7686"


def request(path: str, data=None):
    req = urllib.request.Request(base + path, headers=headers, data=data)
    if data is not None:
        req.add_header("Content-Type", "application/json")
    return urllib.request.urlopen(req, timeout=10)


def current_status() -> str:
    try:
        with request(f"/api/tabs/{tab}/status") as resp:
            return json.load(resp).get("status", "Missing")
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return "Missing"
        raise


def confirm_question() -> None:
    try:
        with request(f"/api/tabs/{tab}/text") as resp:
            text = json.load(resp).get("text", "")
    except urllib.error.HTTPError:
        text = ""
    with open(question_log, "a", encoding="utf-8") as sink:
        sink.write("\n".join(text.splitlines()[-8:]))
        sink.write("\n")
        sink.write("----\n")
    body = json.dumps({"actions": [{"type": "key", "key": "enter"}]}).encode()
    with request(f"/api/tabs/{tab}/send", body):
        pass


def complete(status: str) -> bool:
    if status == "Missing":
        print("missing")
        sys.exit(1)
    if mode == "busy":
        return status != "Idle"
    return status == "Idle"


status = current_status()
if status == "Question":
    confirm_question()
    status = current_status()
if complete(status):
    print(status.lower())
    sys.exit(0)

# Poll /status, do NOT wait on /api/events/stream: that stream carries
# chat-stream-preview only and never emits tab-status-changed, so every
# event-driven wait here burned its full timeout instead of observing readiness.
deadline = time.monotonic() + timeout_s
while time.monotonic() < deadline:
    try:
        status = current_status()
    except (urllib.error.URLError, socket.timeout):
        time.sleep(POLL_INTERVAL_S)
        continue
    if status == "Question":
        confirm_question()
        time.sleep(POLL_INTERVAL_S)
        continue
    if complete(status):
        print(status.lower())
        sys.exit(0)
    time.sleep(POLL_INTERVAL_S)
print("timeout")
sys.exit(1 if mode == "idle" else 0)
PYEOF
}

prompt_count() {
  python3 - "$QUESTIONS" <<'PYEOF'
import json
import sys
print(len(json.load(open(sys.argv[1], encoding="utf-8"))["questions"]))
PYEOF
}

prompt_at() {
  python3 - "$QUESTIONS" "$1" <<'PYEOF'
import json
import sys
questions = json.load(open(sys.argv[1], encoding="utf-8"))["questions"]
print(questions[int(sys.argv[2]) - 1]["prompt"], end="")
PYEOF
}

epoch_ms() {
  python3 - <<'PYEOF'
import time
print(int(time.time() * 1000))
PYEOF
}

echo "== provider=$PROVIDER tab=$TAB out=$OUT_DIR"

echo "-- waiting for initial idle"
wait_status idle 120 >/dev/null || { echo "tab never settled before start"; exit 1; }

if [ -n "${COLS:-}" ] && [ -n "${ROWS:-}" ]; then
  echo "-- resize to ${COLS}x${ROWS}"
  dev_post "/api/tabs/$TAB/resize" "{\"cols\":$COLS,\"rows\":$ROWS}" >/dev/null
fi

echo "-- capture start"
CAPTURED_AT_MS="$(epoch_ms)"
dc tab capture start --tab "$TAB" --out "$CAST_PATH" --json | tee "$OUT_DIR/capture-start.json"; echo
CAPTURE_ACTIVE=1
printf '{"captured_at_ms":%s,"tab":"%s","provider":"%s","cast":"%s","questions":"%s","cols":"%s","rows":"%s"}\n' \
  "$CAPTURED_AT_MS" "$TAB" "$PROVIDER" "$CAST_PATH" "$(basename "$QUESTIONS")" "${COLS:-default}" "${ROWS:-default}" > "$OUT_DIR/meta.json"

echo "-- sidecar start"
python3 "$REPO_ROOT/testbench/streaming/record-chat-timeline.py" \
  --tab "$TAB" --out "$OUT_DIR/chat-timeline.ndjson" --duration-s 1800 &
SIDECAR_PID=$!

curl -sN -H "$CODETERM_DEV_AUTH_HEADER" "http://127.0.0.1:7686/api/events/stream" \
  | grep --line-buffered '"chat-stream-preview"' > "$OUT_DIR/preview-events.ndjson" &
SSE_PID=$!

cleanup() {
  if [ "$CAPTURE_ACTIVE" -eq 1 ]; then
    dc tab capture stop --tab "$TAB" --json >/dev/null 2>&1 || true
  fi
  if [ -n "$SIDECAR_PID" ]; then
    kill "$SIDECAR_PID" 2>/dev/null || true
  fi
  kill "${SSE_PID:-0}" 2>/dev/null || true
}
trap cleanup EXIT

COUNT="$(prompt_count)"
for ((n = 1; n <= COUNT; n++)); do
  prompt="$(prompt_at "$n")"
  echo "-- prompt $n: ${prompt:0:60}..."
  prompt_at "$n" | dc send --tab "$TAB" --stdin --actions enter >/dev/null
  wait_status busy 30 >/dev/null || true
  settled="$(wait_status idle 300)" || echo "WARN prompt $n never settled ($settled)"
  echo "   settled: ${settled:-idle}"
done

echo "-- capture stop"
dc tab capture stop --tab "$TAB" --json | tee "$OUT_DIR/capture-stop.json"; echo
CAPTURE_ACTIVE=0

kill "$SIDECAR_PID" 2>/dev/null || true
SIDECAR_PID=""
kill "${SSE_PID:-0}" 2>/dev/null || true
SSE_PID=""
trap - EXIT
echo "== done $PROVIDER"
