#!/usr/bin/env bash
# Dev-only REST liveness oracle. The outer deadline is failure-only; every
# successful transition is proven by a CLI/API acknowledgement.

set -euo pipefail

if [ "${CODETERM_API_LIVENESS_ORACLE_INNER:-}" != "1" ]; then
  export CODETERM_API_LIVENESS_ORACLE_INNER=1
  exec timeout --signal=TERM 15s bash "$0"
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck source=/dev/null
source "$ROOT/evals/_lib/lib.sh"

RUN_ROOT="$(mktemp -d "$ROOT/target-check/evals/api-liveness-mesh-load.XXXXXX")"
if is_windows; then
  FIXTURE_CWD="$(cygpath -w "$RUN_ROOT")"
else
  FIXTURE_CWD="$RUN_ROOT"
fi
WORKSPACE_ID=""
TAB_ID=""
HEARTBEAT_TAB_ID=""
TRANSCRIPT_PATH=""
if is_windows; then
  REPORT_MARKER_PATH="$(cygpath -w "$RUN_ROOT/report-deliveries.txt")"
  HEARTBEAT_MARKER_PATH="$(cygpath -w "$RUN_ROOT/heartbeat-deliveries.txt")"
else
  REPORT_MARKER_PATH="$RUN_ROOT/report-deliveries.txt"
  HEARTBEAT_MARKER_PATH="$RUN_ROOT/heartbeat-deliveries.txt"
fi
START_NS="$(python -c 'import time; print(time.monotonic_ns())')"

diagnose_failure() {
  python - <<'PY'
import socket

try:
    with socket.create_connection(("127.0.0.1", 7686), timeout=0.5) as stream:
        stream.settimeout(0.5)
        stream.sendall(b"GET /api/health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        response = stream.recv(64)
        if b"200 OK" in response:
            print("DIAGNOSTIC: listener and REST workers still answer real health; inspect the failed oracle assertion", flush=True)
        else:
            print(f"DIAGNOSTIC: listener answered health without success: {response!r}", flush=True)
except socket.timeout:
    print("DIAGNOSTIC: listener remains bound; an unanswered health request classifies REST workers as blocked", flush=True)
except OSError as error:
    print(f"DIAGNOSTIC: listener exit or bind ownership failure: {error}", flush=True)
PY
  local log="$HOME/.codeterm-dev/logs/codeterm.log"
  if [ -f "$log" ]; then
    grep -E '\[chat_start\.bounded\]|\[liveness\]|\[LOCK-HOLD\]|\[SLOW-IPC\]|REST API' "$log" \
      | tail -20 >&2 || true
  fi
  if [ -n "$TAB_ID" ]; then
    dc tab get-text --tab "$TAB_ID" >&2 || true
  fi
  if [ -n "$HEARTBEAT_TAB_ID" ]; then
    dc tab get-text --tab "$HEARTBEAT_TAB_ID" >&2 || true
  fi
}

cleanup() {
  if [ -n "$TAB_ID" ]; then
    dc tab close "$TAB_ID" >/dev/null 2>&1 || true
  fi
  if [ -n "$HEARTBEAT_TAB_ID" ]; then
    dc tab close "$HEARTBEAT_TAB_ID" >/dev/null 2>&1 || true
  fi
  if [ -n "$WORKSPACE_ID" ]; then
    dc workspace close "$WORKSPACE_ID" >/dev/null 2>&1 || true
  fi
  python - "$RUN_ROOT" "$TRANSCRIPT_PATH" <<'PY'
import pathlib
import shutil
import sys

run_root = pathlib.Path(sys.argv[1]).resolve()
transcript = pathlib.Path(sys.argv[2]) if sys.argv[2] else None
if transcript and transcript.is_file():
    transcript.unlink()
    try:
        transcript.parent.rmdir()
    except OSError:
        pass
if run_root.name.startswith("api-liveness-mesh-load."):
    shutil.rmtree(run_root, ignore_errors=True)
PY
}
trap 'rc=$?; trap - EXIT; if [ "$rc" -ne 0 ]; then diagnose_failure; fi; cleanup; exit "$rc"' EXIT

health="$(curl -sS --fail --max-time 1 http://127.0.0.1:7686/api/health)"
build="$(printf '%s' "$health" | python -c 'import json,sys; print(json.load(sys.stdin)["build"])')"

workspace="$(dc workspace create "WS-api-liveness-$$" --cwd "$FIXTURE_CWD" --json)"
WORKSPACE_ID="$(printf '%s' "$workspace" | python -c 'import json,sys; print(json.load(sys.stdin)["id"])')"
tab="$(dc tab new --workspace "$WORKSPACE_ID" --cwd "$FIXTURE_CWD" \
  --title "api-liveness-report-$$" --eager --json)"
TAB_ID="$(printf '%s' "$tab" | python -c 'import json,sys; d=json.load(sys.stdin); print(d.get("tab_id") or d["id"])')"
heartbeat_tab="$(dc tab new --workspace "$WORKSPACE_ID" --cwd "$FIXTURE_CWD" \
  --title "api-liveness-heartbeat-$$" --eager --json)"
HEARTBEAT_TAB_ID="$(printf '%s' "$heartbeat_tab" | python -c 'import json,sys; d=json.load(sys.stdin); print(d.get("tab_id") or d["id"])')"

TRANSCRIPT_PATH="$(python - "$FIXTURE_CWD" <<'PY'
import json
import pathlib
import sys

cwd = sys.argv[1]
session_id = "api-liveness-load-oracle"
encoded = "".join(char if char.isalnum() else "-" for char in cwd)
path = pathlib.Path.home() / ".claude" / "projects" / encoded / f"{session_id}.jsonl"
path.parent.mkdir(parents=True, exist_ok=True)
total = 476 * 1024 * 1024
window = 4 * 1024 * 1024
record = {
    "type": "assistant",
    "uuid": "api-liveness-tail",
    "cwd": cwd,
    "message": {
        "content": [{"type": "text", "text": "ORACLE_TRANSCRIPT_TAIL"}],
        "model": "oracle",
    },
}
with path.open("wb") as stream:
    stream.truncate(total)
    stream.seek(total - window)
    stream.write(b"\n")
    stream.write(json.dumps(record, separators=(",", ":")).encode() + b"\n")
print(path)
PY
)"

signed="$(dev_get /api/mesh/debug/sign)"
SIGNED_TOKEN="$(printf '%s' "$signed" | python -c 'import json,sys; print(json.load(sys.stdin)["token"])')"

python - "$(dev_token)" "$SIGNED_TOKEN" "$FIXTURE_CWD" \
  "$TAB_ID" "$HEARTBEAT_TAB_ID" "$TRANSCRIPT_PATH" \
  "$REPORT_MARKER_PATH" "$HEARTBEAT_MARKER_PATH" <<'PY'
import concurrent.futures
import json
import pathlib
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

(
    api_token,
    signed_token,
    cwd,
    tab_id,
    heartbeat_tab_id,
    transcript_path,
    report_marker_path,
    heartbeat_marker_path,
) = sys.argv[1:]
base = "http://127.0.0.1:7686"
barrier = threading.Barrier(10)

def request(name, path, token, deadline, body=None):
    barrier.wait()
    headers = {"Authorization": f"Bearer {token}"}
    data = None
    if body is not None:
        headers["Content-Type"] = "application/json"
        data = json.dumps(body, separators=(",", ":")).encode()
    started = time.monotonic()
    req = urllib.request.Request(base + path, headers=headers, data=data)
    with urllib.request.urlopen(req, timeout=deadline) as response:
        payload = response.read()
        status = response.status
    elapsed = time.monotonic() - started
    if elapsed > deadline:
        raise AssertionError(f"{name} took {elapsed:.3f}s, limit {deadline:.3f}s")
    return name, status, payload, elapsed

def transcript(index):
    query = urllib.parse.urlencode({
        "provider": "claude",
        "cwd": cwd,
        "session_id": "api-liveness-load-oracle",
    })
    name, status, payload, elapsed = request(
        f"transcript-{index}", f"/api/sessions/transcript?{query}", api_token, 8.0
    )
    if status != 200 or b"ORACLE_TRANSCRIPT_TAIL" not in payload:
        raise AssertionError(f"{name} did not return the bounded sparse tail")
    return name, status, payload, elapsed

def append_command(marker_path, label, output):
    marker = repr(marker_path)
    return (
        "python -c \"from pathlib import Path;"
        f"p=Path({marker});p.open('a').write({label!r}+chr(10));"
        f"print({output!r})\""
    )

report = {
    "method": "POST",
    "path": f"/api/tabs/{tab_id}/send",
    "delivery_id": f"api-liveness-report-{tab_id}",
    "body": {
        "text": append_command(report_marker_path, "report", "ORACLE_REPORT_OUTPUT"),
        "actions": [{"type": "key", "key": "enter"}],
        "source_tab": "remote-worker",
        "source_label": "load-worker",
        "source_model": "oracle",
        "kind": "report",
    },
}
heartbeat = {
    "method": "POST",
    "path": f"/api/tabs/{heartbeat_tab_id}/send",
    "delivery_id": f"api-liveness-heartbeat-{heartbeat_tab_id}",
    "body": {
        "text": append_command(
            heartbeat_marker_path, "heartbeat", "ORACLE_HEARTBEAT_OUTPUT"
        ),
        "actions": [{"type": "key", "key": "enter"}],
        "source_tab": "remote-worker",
        "source_label": "load-worker",
        "source_model": "oracle",
        "kind": "heartbeat",
    },
}
structured_error = {
    "method": "POST",
    "path": "/api/tabs/api-liveness-missing/send",
    "delivery_id": f"api-liveness-structured-error-{tab_id}",
    "body": {"text": "must-not-land"},
}

jobs = [
    lambda index=index: transcript(index) for index in range(4)
] + [
    lambda: request("health", "/api/health", api_token, 1.0),
    lambda: request("tab", "/api/tabs?all_workspaces=true", api_token, 1.0),
    lambda: request("report-owner", "/api/mesh/relay", signed_token, 2.0, report),
    lambda: request("report-duplicate", "/api/mesh/relay", signed_token, 2.0, report),
    lambda: request("heartbeat", "/api/mesh/relay", signed_token, 2.0, heartbeat),
    lambda: request(
        "structured-error", "/api/mesh/relay", signed_token, 2.0, structured_error
    ),
]

with concurrent.futures.ThreadPoolExecutor(max_workers=len(jobs)) as executor:
    results = list(executor.map(lambda job: job(), jobs))

by_name = {name: (status, payload, elapsed) for name, status, payload, elapsed in results}
health_status, health_payload, health_elapsed = by_name["health"]
if health_status != 200 or json.loads(health_payload).get("status") != "ok":
    raise AssertionError("health did not return a real successful response")
tab_status, tab_payload, tab_elapsed = by_name["tab"]
observed_tabs = {item["id"] for item in json.loads(tab_payload)}
if (
    tab_status != 200
    or tab_id not in observed_tabs
    or heartbeat_tab_id not in observed_tabs
):
    raise AssertionError("tab observation did not contain the load tab")

owner = json.loads(by_name["report-owner"][1])
duplicate = json.loads(by_name["report-duplicate"][1])
if owner != duplicate or not owner.get("ok"):
    raise AssertionError("duplicate report relay did not replay the owner acknowledgement")
heartbeat_response = json.loads(by_name["heartbeat"][1])
if not heartbeat_response.get("ok"):
    raise AssertionError(f"heartbeat-shaped relay failed: {heartbeat_response}")
error_response = json.loads(by_name["structured-error"][1])
error = (error_response.get("data") or {}).get("error") or {}
if (
    error_response.get("ok") is not False
    or error.get("code") != "tab_not_reachable_on_peer"
    or error.get("bytes_written") is not False
):
    raise AssertionError(f"structured delivery error was lost: {error_response}")

for name in ("report-owner", "report-duplicate", "heartbeat", "structured-error"):
    if by_name[name][2] > 2.0:
        raise AssertionError(f"{name} acknowledgement exceeded 2s")

print(
    "latency "
    f"health={health_elapsed:.3f}s tab={tab_elapsed:.3f}s "
    f"report={by_name['report-owner'][2]:.3f}s "
    f"duplicate={by_name['report-duplicate'][2]:.3f}s "
    f"heartbeat={by_name['heartbeat'][2]:.3f}s "
    f"error={by_name['structured-error'][2]:.3f}s"
)
PY

report_fence="$(dc tab exec --tab "$TAB_ID" --timeout 2000 \
  "python -c 'print(\"ORACLE_REPORT_FENCE\")'")"
heartbeat_fence="$(dc tab exec --tab "$HEARTBEAT_TAB_ID" --timeout 2000 \
  "python -c 'print(\"ORACLE_HEARTBEAT_FENCE\")'")"
grep -q "ORACLE_REPORT_FENCE" <<<"$report_fence"
grep -q "ORACLE_HEARTBEAT_FENCE" <<<"$heartbeat_fence"

deliveries="$(python - "$REPORT_MARKER_PATH" "$HEARTBEAT_MARKER_PATH" <<'PY'
import pathlib
import sys

report = pathlib.Path(sys.argv[1]).read_text().splitlines()
heartbeat = pathlib.Path(sys.argv[2]).read_text().splitlines()
if report != ["report"] or heartbeat != ["heartbeat"]:
    raise SystemExit(
        f"duplicate delivery detected: report={report} heartbeat={heartbeat}"
    )
print("report,heartbeat")
PY
)"
report_screen="$(dc tab get-text --tab "$TAB_ID")"
heartbeat_screen="$(dc tab get-text --tab "$HEARTBEAT_TAB_ID")"
grep -q "ORACLE_REPORT_OUTPUT" <<<"$report_screen"
grep -q "ORACLE_HEARTBEAT_OUTPUT" <<<"$heartbeat_screen"

END_NS="$(python -c 'import time; print(time.monotonic_ns())')"
ELAPSED_MS="$(( (END_NS - START_NS) / 1000000 ))"
[ "$ELAPSED_MS" -le 15000 ] || {
  echo "FAIL: oracle exceeded 15s (${ELAPSED_MS}ms)" >&2
  exit 1
}

echo "PASS: build=$build sparse_bytes=$((476 * 1024 * 1024)) deliveries=$deliveries elapsed=${ELAPSED_MS}ms"
echo "PASS: real sparse transcript owner stayed on bounded spawn_blocking work; health/tab and relay acknowledgements remained responsive"
