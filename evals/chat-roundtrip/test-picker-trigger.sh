#!/usr/bin/env bash
# WHAT/WHY: Verifies two raw requests; an Enter action starts picker-clearing recovery.
# ORACLE: Captured JSON is raw /model then one raw CR, on the configured API port.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
RUN_SH="$ROOT/evals/chat-roundtrip/run.sh"
PY="$(command -v python3 || command -v python)"
CAPTURE="$(mktemp)"
trap 'rm -f "$CAPTURE"' EXIT

source <(awk '
  /^send_raw_keys\(\) \{/ { capture = 1 }
  capture { print }
  capture && /^}/ { exit }
' "$RUN_SH")

curl() {
  local body="" data="" url=""
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --data)
        shift
        data="${1:-}"
        ;;
      http://*) url="$1" ;;
    esac
    shift
  done
  if [ "$data" = "@-" ]; then
    body="$(command cat)"
  else
    body="$data"
  fi
  BODY="$body" URL="$url" "$PY" -c '
import json
import os

print(json.dumps({"body": os.environ["BODY"], "url": os.environ["URL"]}))
' >> "$CAPTURE"
}

export CODETERM_API_PORT=43197
export CODETERM_DEV_AUTH_HEADER="Authorization: Bearer fixture"
export CAPTURE PY

send_raw_keys "picker-tab" "/model"

"$PY" - "$CAPTURE" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as capture:
    requests = [json.loads(line) for line in capture if line.strip()]

if len(requests) != 2:
    raise SystemExit(f"expected exactly two requests, got {len(requests)}")

expected_url = "http://127.0.0.1:43197/api/tabs/picker-tab/send"
for index, request in enumerate(requests, start=1):
    if request["url"] != expected_url:
        raise SystemExit(
            f"request {index}: expected configured API URL {expected_url!r}, "
            f"got {request['url']!r}"
        )

payloads = [json.loads(request["body"]) for request in requests]
if payloads[0] != {"text": "/model", "raw": True}:
    raise SystemExit(f"request 1: expected raw /model, got {payloads[0]!r}")
if payloads[1] != {"text": "\r", "raw": True}:
    raise SystemExit(f"request 2: expected one raw carriage return, got {payloads[1]!r}")
if len(payloads[1]["text"]) != 1 or ord(payloads[1]["text"]) != 13:
    raise SystemExit("request 2: text is not exactly one carriage return")

print("picker trigger request shape: PASS")
PY
