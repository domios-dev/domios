#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
source "$ROOT/evals/_lib/lib.sh"

PAYLOAD_SIZES="${PAYLOAD_SIZES:-4096 16384 36435 65536}"
LONG_PAYLOAD_CHARS="${LONG_PAYLOAD_CHARS:-36435}"
EVENT_TIMEOUT="${EVENT_TIMEOUT:-90}"
LOG_EVENT_TIMEOUT="${LOG_EVENT_TIMEOUT:-15}"
TURN_START_TIMEOUT="${TURN_START_TIMEOUT:-15}"
RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)-$$"
OUT_DIR="${CODETERM_LONG_PASTE_OUT:-$ROOT/target-check/evals/long-paste-submit/$RUN_ID}"
BENCH="$ROOT/evals/long-paste-submit"
LOG_FILE="$CODETERM_DEV_DATA_DIR/logs/codeterm.log"
SHORT_TOKEN="SHORT_PASTE_${RUN_ID}"
RECOVERY_TOKEN="RECOVERY_PROBE_${RUN_ID}"
SHORT_PAYLOAD="$OUT_DIR/short-payload.txt"
RECOVERY_PAYLOAD="$OUT_DIR/recovery-payload.txt"
LONG_PAYLOAD="$OUT_DIR/payload-${LONG_PAYLOAD_CHARS}.txt"
LONG_TOKEN="SIZE_${LONG_PAYLOAD_CHARS}_${RUN_ID}"
WORKSPACE_ID=""
CREATED_TABS=()
GATE_ACTIVE=0
GATE_CONNECTED=0
SPAWN_TAB=""
SPAWN_PTY=""
GATE_PORT=""
SEND_ID=""

mkdir -p "$OUT_DIR"

json_value() {
  python3 -c '
import json, sys
keys = sys.argv[1:]
def find(value):
    if isinstance(value, dict):
        for key in keys:
            if value.get(key) not in (None, ""):
                return value[key]
        for child in value.values():
            found = find(child)
            if found not in (None, ""):
                return found
    if isinstance(value, list):
        for child in value:
            found = find(child)
            if found not in (None, ""):
                return found
    return None
value = find(json.load(sys.stdin))
if value in (None, ""):
    raise SystemExit(1)
print(value)
' "$@"
}

release_gate() {
  if [ "$GATE_ACTIVE" -ne 1 ]; then
    return
  fi
  if [ "$GATE_CONNECTED" -eq 1 ]; then
    python3 "$BENCH/gate.py" release "$GATE_PORT" "$EVENT_TIMEOUT" 2>/dev/null || true
    read -r _ <&"${GATEPROC[0]}" 2>/dev/null || true
  else
    kill "$GATEPROC_PID" 2>/dev/null || true
  fi
  wait "$GATEPROC_PID" 2>/dev/null || true
  GATE_ACTIVE=0
  GATE_CONNECTED=0
  GATE_PORT=""
}

cleanup() {
  set +e
  release_gate
  for tab in "${CREATED_TABS[@]:-}"; do
    [ -z "$tab" ] || dc tab close --tab "$tab" >/dev/null 2>&1
  done
  [ -z "$WORKSPACE_ID" ] || dc workspace close "$WORKSPACE_ID" --force >/dev/null 2>&1
}
trap cleanup EXIT INT TERM

status_json() {
  dev_get "/api/tabs/$1/status"
}

status_value() {
  status_json "$1" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("status", ""))'
}

capture_screen() {
  dc tab get-text --tab "$1"
}

wait_for_tui() {
  local tab="$1" case_dir="$2"
  local url="$(dev_api_url)/api/output/stream?tab_id=$tab"
  python3 "$BENCH/wait_sse.py" "$url" '\x1b\[\?2004h' "$EVENT_TIMEOUT" \
    >"$case_dir/tui-ready-event.txt" 2>"$case_dir/tui-ready-event.err"
  local log_offset status
  log_offset="$(wc -c <"$LOG_FILE")"
  status="$(status_value "$tab")"
  if [ "$status" != "Idle" ]; then
    python3 "$BENCH/wait_log.py" "$LOG_FILE" "$log_offset" \
      "\\[idle-poll\\].* Idle \\(tab=$tab\\)" "$EVENT_TIMEOUT" \
      >"$case_dir/status-idle-event.txt" 2>"$case_dir/status-idle-event.err"
    status="$(status_value "$tab")"
  fi
  status_json "$tab" >"$case_dir/status-ready.json"
  if [ "$status" != "Idle" ]; then
    capture_screen "$tab" >"$case_dir/screen-ready-mismatch.txt" || true
    echo "FAIL: tab $tab reached bracketed-paste mode but settled status=$status, wanted Idle" >&2
    return 1
  fi
}

spawn_codex() {
  local label="$1" case_dir="$2" spawn tab inspect pty recorded_provider
  spawn="$(dc agent spawn codex --workspace "$WORKSPACE_ID" --cwd "$ROOT" \
    --title "pty-long-paste-$label" --description "isolated long-paste submit bench" \
    --non-user-facing --json)"
  printf '%s\n' "$spawn" >"$case_dir/spawn.json"
  tab="$(printf '%s' "$spawn" | json_value tab_id id)"
  CREATED_TABS+=("$tab")
  wait_for_tui "$tab" "$case_dir"
  inspect="$(dc tab inspect --tab "$tab" --json)"
  printf '%s\n' "$inspect" >"$case_dir/inspect.json"
  pty="$(printf '%s' "$inspect" | json_value pty_id)"
  recorded_provider="$(printf '%s' "$inspect" | json_value agent_type agentType provider_id provider || true)"
  printf 'requested_provider=codex\nrecorded_provider=%s\npty=%s\n' \
    "${recorded_provider:-unresolved}" "$pty" >"$case_dir/provider-selection.txt"
  SPAWN_TAB="$tab"
  SPAWN_PTY="$pty"
}

agent_gate_path() {
  if is_windows; then
    cygpath -w "$BENCH/gate.py"
  else
    printf '%s\n' "$BENCH/gate.py"
  fi
}

agent_python() {
  if is_windows; then
    printf 'python\n'
  else
    printf 'python3\n'
  fi
}

start_gate() {
  local tab="$1" case_dir="$2" port event command
  coproc GATEPROC { python3 "$BENCH/gate.py" server "$EVENT_TIMEOUT"; }
  GATE_ACTIVE=1
  GATE_CONNECTED=0
  if ! read -r port <&"${GATEPROC[0]}"; then
    echo "FAIL: gate server did not publish a port" >&2
    return 1
  fi
  port="${port%$'\r'}"
  command="$(agent_python) \"$(agent_gate_path)\" client $port $EVENT_TIMEOUT"
  printf 'Run this exact command immediately as a foreground terminal command:\n%s\nDo not substitute another command and do not finish this turn until it exits.\n' \
    "$command" >"$case_dir/gate-task.txt"
  dc send --tab "$tab" --require-agent --from-file "$case_dir/gate-task.txt" --actions enter --json \
    >"$case_dir/gate-send.json" 2>"$case_dir/gate-send.err"
  if ! read -r event <&"${GATEPROC[0]}"; then
    echo "FAIL: Codex did not connect to the gate within ${EVENT_TIMEOUT}s" >&2
    return 1
  fi
  event="${event%$'\r'}"
  if [ "$event" != "CONNECTED" ]; then
    echo "FAIL: gate server reported '$event'" >&2
    return 1
  fi
  GATE_CONNECTED=1
  GATE_PORT="$port"
  status_json "$tab" >"$case_dir/status-mid-turn.json"
  if [ "$(status_value "$tab")" != "Working" ]; then
    capture_screen "$tab" >"$case_dir/screen-mid-turn-mismatch.txt" || true
    echo "FAIL: gate connected but tab $tab is not Working" >&2
    return 1
  fi
  capture_screen "$tab" >"$case_dir/screen-mid-turn-precondition.txt"
}

wait_exact_confirmation() {
  local tab="$1" send_id="$2" log_offset="$3" out="$4"
  python3 "$BENCH/wait_log.py" "$LOG_FILE" "$log_offset" \
    "\\[chat-send\\] send_id=$send_id tab=$tab .*confirmed by .*turn evidence" \
    "$TURN_START_TIMEOUT" >"$out" 2>"$out.err"
}

observe_after_turn_end() {
  local tab="$1" pty="$2" case_dir="$3" send_id="$4" second_send_id="${5:-}"
  local log_offset status_pid confirm_pid second_confirm_pid=""
  log_offset="$(wc -c <"$LOG_FILE")"
  python3 "$BENCH/wait_log.py" "$LOG_FILE" "$log_offset" \
    "\\[status\\].*$pty Idle -> Working" "$TURN_START_TIMEOUT" \
    >"$case_dir/turn-start-after-release.txt" \
    2>"$case_dir/turn-start-after-release.err" &
  status_pid=$!
  wait_exact_confirmation "$tab" "$send_id" "$log_offset" \
    "$case_dir/exact-confirmation-after-release.txt" &
  confirm_pid=$!
  if [ -n "$second_send_id" ]; then
    wait_exact_confirmation "$tab" "$second_send_id" "$log_offset" \
      "$case_dir/second-exact-confirmation-after-release.txt" &
    second_confirm_pid=$!
  fi
  release_gate
  if wait "$status_pid"; then
    printf 'TURN START OBSERVED AFTER GATE RELEASE\n' \
      >"$case_dir/turn-start-observation.txt"
  else
    printf 'NO TURN START OBSERVED WITHIN %ss AFTER GATE RELEASE\n' \
      "$TURN_START_TIMEOUT" >"$case_dir/turn-start-observation.txt"
  fi
  if wait "$confirm_pid"; then
    printf 'EXACT TURN CONFIRMATION OBSERVED AFTER GATE RELEASE\n' \
      >"$case_dir/exact-confirmation-observation.txt"
  else
    printf 'NO EXACT TURN CONFIRMATION WITHIN %ss AFTER GATE RELEASE\n' \
      "$TURN_START_TIMEOUT" >"$case_dir/exact-confirmation-observation.txt"
  fi
  if [ -n "$second_confirm_pid" ]; then
    wait "$second_confirm_pid" || true
  fi
  capture_screen "$tab" >"$case_dir/screen-after-turn-end.txt" || true
  status_json "$tab" >"$case_dir/status-after-turn-end.json" || true
  dc tab chat --tab "$tab" --json >"$case_dir/transcript-after-turn-end.json" \
    2>"$case_dir/transcript-after-turn-end.err" || true
  dev_get "/api/tabs/$tab/chat-send" >"$case_dir/ledger-after-turn-end.json" || true
}

start_send() {
  local tab="$1" payload="$2" prefix="$3" status response
  python3 -c '
import json, pathlib, sys
print(json.dumps({"text": pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"),
                  "client_token": sys.argv[2]}))
' "$payload" "bench-${RUN_ID}-${prefix##*/}" >"$prefix.request.json"
  dev_env
  dev_assert_target
  if status="$(curl --silent --show-error --max-time 30 -o "$prefix.json" \
      -w '%{http_code}' -X POST -H "$CODETERM_DEV_AUTH_HEADER" \
      -H 'Content-Type: application/json' --data-binary "@$prefix.request.json" \
      "$(dev_api_url)/api/tabs/$tab/chat-send" 2>"$prefix.err")" \
      && [[ "$status" == 2* ]]; then
    printf '0\n' >"$prefix.exit"
  else
    printf '%s\n' "${status:-curl-failed}" >"$prefix.exit"
    cat "$prefix.err" >&2 || true
    cat "$prefix.json" >&2 || true
    return 1
  fi
  SEND_ID="$(json_value send_id <"$prefix.json")"
}

wait_first_key() {
  local pty="$1" offset="$2" out="$3"
  python3 "$BENCH/wait_log.py" "$LOG_FILE" "$offset" \
    "\\[submit-audit\\] key-write pty=$pty .*queue_result=ok" "$LOG_EVENT_TIMEOUT" \
    >"$out" 2>"$out.err"
}

wait_key_burst() {
  local pty="$1" offset="$2" out="$3" count="${4:-3}"
  python3 "$BENCH/wait_log.py" "$LOG_FILE" "$offset" \
    "\\[submit-audit\\] key-write pty=$pty .*queue_result=ok" \
    "$LOG_EVENT_TIMEOUT" "$count" >"$out" 2>"$out.err"
}

observe_idle_outcome() {
  local tab="$1" pty="$2" case_dir="$3" send_id="$4" log_offset="$5"
  local status_pid confirm_pid
  python3 "$BENCH/wait_log.py" "$LOG_FILE" "$log_offset" \
    "\\[status\\].*$pty Idle -> Working" "$TURN_START_TIMEOUT" \
    >"$case_dir/turn-start.txt" 2>"$case_dir/turn-start.err" &
  status_pid=$!
  wait_exact_confirmation "$tab" "$send_id" "$log_offset" \
    "$case_dir/exact-confirmation.txt" &
  confirm_pid=$!
  if wait "$status_pid"; then
    printf 'TURN START OBSERVED\n' >"$case_dir/turn-start-observation.txt"
  else
    printf 'NO TURN START OBSERVED WITHIN %ss\n' "$TURN_START_TIMEOUT" \
      >"$case_dir/turn-start-observation.txt"
  fi
  if wait "$confirm_pid"; then
    printf 'EXACT TURN CONFIRMATION OBSERVED\n' \
      >"$case_dir/exact-confirmation-observation.txt"
  else
    printf 'NO EXACT TURN CONFIRMATION WITHIN %ss\n' "$TURN_START_TIMEOUT" \
      >"$case_dir/exact-confirmation-observation.txt"
  fi
  dev_get "/api/tabs/$tab/chat-send" >"$case_dir/ledger.json" || true
}

finish_capture() {
  local tab="$1" pty="$2" log_offset="$3" case_dir="$4" source_bytes="$5"
  dc tab capture stop --tab "$tab" --json >"$case_dir/capture-stop.json"
  python3 "$BENCH/summarize_cast.py" "$case_dir/trace.cast" \
    --log "$LOG_FILE" --log-offset "$log_offset" --pty "$pty" \
    --expected-source-bytes "$source_bytes" \
    >"$case_dir/sequence.txt"
}

print_case() {
  local label="$1" case_dir="$2"
  echo "=== $label ==="
  cat "$case_dir/provider-selection.txt"
  echo "--- precondition ---"
  cat "$case_dir/screen-precondition.txt"
  echo "--- screen at first successful submit-key write ---"
  cat "$case_dir/screen-at-first-key.txt"
  echo "--- screen after third successful submit-key write ---"
  cat "$case_dir/screen-after-recovery.txt"
  echo "--- send result ---"
  cat "$case_dir/send.exit"
  cat "$case_dir/send.json" 2>/dev/null || true
  cat "$case_dir/send.err" 2>/dev/null || true
  echo "--- outcome signals ---"
  cat "$case_dir/turn-start-observation.txt" 2>/dev/null || true
  cat "$case_dir/turn-start.txt" 2>/dev/null || true
  cat "$case_dir/turn-start.err" 2>/dev/null || true
  cat "$case_dir/exact-confirmation-observation.txt" 2>/dev/null || true
  cat "$case_dir/exact-confirmation.txt" 2>/dev/null || true
  cat "$case_dir/exact-confirmation.txt.err" 2>/dev/null || true
  echo "--- transcript observation after third key ---"
  cat "$case_dir/transcript-summary.txt"
  if [ -f "$case_dir/turn-start-observation.txt" ]; then
    echo "--- after the blocked control turn ended, with no further input ---"
    cat "$case_dir/turn-start-observation.txt"
    cat "$case_dir/turn-start-after-release.txt" 2>/dev/null || true
    cat "$case_dir/turn-start-after-release.err" 2>/dev/null || true
    cat "$case_dir/exact-confirmation-observation.txt"
    cat "$case_dir/exact-confirmation-after-release.txt" 2>/dev/null || true
    cat "$case_dir/exact-confirmation-after-release.txt.err" 2>/dev/null || true
    cat "$case_dir/screen-after-turn-end.txt"
    cat "$case_dir/transcript-after-turn-summary.txt"
  fi
  echo "--- captured PTY sequence ---"
  cat "$case_dir/sequence.txt"
  echo
}

run_case() {
  local label="$1" mode="$2" payload="$3" token="$4" case_dir="$OUT_DIR/$label"
  local tab pty log_offset
  mkdir -p "$case_dir"
  spawn_codex "$label" "$case_dir"
  tab="$SPAWN_TAB"
  pty="$SPAWN_PTY"
  if [ "$mode" = "mid-turn" ]; then
    start_gate "$tab" "$case_dir"
    cp "$case_dir/screen-mid-turn-precondition.txt" "$case_dir/screen-precondition.txt"
  else
    capture_screen "$tab" >"$case_dir/screen-precondition.txt"
  fi
  dc tab capture start --tab "$tab" --out "$case_dir/trace.cast" --json \
    >"$case_dir/capture-start.json"
  log_offset="$(wc -c <"$LOG_FILE")"
  start_send "$tab" "$payload" "$case_dir/send"
  local send_id="$SEND_ID"
  if wait_first_key "$pty" "$log_offset" "$case_dir/first-key-event.txt"; then
    capture_screen "$tab" >"$case_dir/screen-at-first-key.txt"
  else
    printf 'NO SUCCESSFUL SUBMIT-KEY WRITE OBSERVED\n' >"$case_dir/screen-at-first-key.txt"
    capture_screen "$tab" >>"$case_dir/screen-at-first-key.txt" || true
  fi
  wait_key_burst "$pty" "$log_offset" "$case_dir/key-burst-events.txt" 3 || true
  capture_screen "$tab" >"$case_dir/screen-after-recovery.txt"
  status_json "$tab" >"$case_dir/status-after-recovery.json"
  dc tab chat --tab "$tab" --json >"$case_dir/transcript.json" \
    2>"$case_dir/transcript.err" || true
  python3 "$BENCH/summarize_transcript.py" "$case_dir/transcript.json" "$token" \
    >"$case_dir/transcript-summary.txt"
  if [ "$mode" = "mid-turn" ]; then
    observe_after_turn_end "$tab" "$pty" "$case_dir" "$send_id"
    python3 "$BENCH/summarize_transcript.py" \
      "$case_dir/transcript-after-turn-end.json" "$token" \
      >"$case_dir/transcript-after-turn-summary.txt"
  else
    observe_idle_outcome "$tab" "$pty" "$case_dir" "$send_id" "$log_offset"
  fi
  finish_capture "$tab" "$pty" "$log_offset" "$case_dir" "$(wc -c <"$payload")"
  print_case "$label" "$case_dir"
  dc tab close --tab "$tab" >/dev/null
}

run_recovery_case() {
  local label="long-mid-turn-next-send" case_dir="$OUT_DIR/$label"
  local tab pty log_offset first_send_id second_send_id
  mkdir -p "$case_dir"
  spawn_codex "$label" "$case_dir"
  tab="$SPAWN_TAB"
  pty="$SPAWN_PTY"
  start_gate "$tab" "$case_dir"
  cp "$case_dir/screen-mid-turn-precondition.txt" "$case_dir/screen-precondition.txt"
  dc tab capture start --tab "$tab" --out "$case_dir/trace.cast" --json \
    >"$case_dir/capture-start.json"
  log_offset="$(wc -c <"$LOG_FILE")"
  start_send "$tab" "$LONG_PAYLOAD" "$case_dir/send"
  first_send_id="$SEND_ID"
  if wait_first_key "$pty" "$log_offset" "$case_dir/first-key-event.txt"; then
    capture_screen "$tab" >"$case_dir/screen-at-first-key.txt"
  else
    printf 'NO SUCCESSFUL SUBMIT-KEY WRITE OBSERVED\n' >"$case_dir/screen-at-first-key.txt"
    capture_screen "$tab" >>"$case_dir/screen-at-first-key.txt" || true
  fi
  start_send "$tab" "$RECOVERY_PAYLOAD" "$case_dir/second-send"
  second_send_id="$SEND_ID"
  wait_key_burst "$pty" "$log_offset" "$case_dir/key-burst-events.txt" 4 || true
  capture_screen "$tab" >"$case_dir/screen-after-recovery.txt"
  status_json "$tab" >"$case_dir/status-after-recovery.json"
  dc tab chat --tab "$tab" --json >"$case_dir/transcript.json" \
    2>"$case_dir/transcript.err" || true
  python3 "$BENCH/summarize_transcript.py" "$case_dir/transcript.json" \
    "$LONG_TOKEN" "$RECOVERY_TOKEN" >"$case_dir/transcript-summary.txt"
  observe_after_turn_end "$tab" "$pty" "$case_dir" "$first_send_id" "$second_send_id"
  python3 "$BENCH/summarize_transcript.py" \
    "$case_dir/transcript-after-turn-end.json" "$LONG_TOKEN" "$RECOVERY_TOKEN" \
    >"$case_dir/transcript-after-turn-summary.txt"
  finish_capture "$tab" "$pty" "$log_offset" "$case_dir" "$LONG_PAYLOAD_CHARS"
  print_case "$label" "$case_dir"
  echo "second send result:"
  cat "$case_dir/second-send.exit"
  cat "$case_dir/second-send.json" 2>/dev/null || true
  cat "$case_dir/second-send.err" 2>/dev/null || true
  dc tab close --tab "$tab" >/dev/null
}

health="$(dev_get /api/health)"
build="$(printf '%s' "$health" | json_value build)"
head="$(git rev-parse HEAD)"
case "$head" in
  "$build"*) ;;
  *) echo "FAIL: dev build $build is not this checkout's HEAD $head" >&2; exit 1 ;;
esac
[ -f "$LOG_FILE" ] || { echo "FAIL: dev log not found: $LOG_FILE" >&2; exit 1; }

dc identify --json >"$OUT_DIR/dev-identify.json"
dc agent providers --json >"$OUT_DIR/providers.json"
workspace="$(dc workspace create "PTY-LONG-PASTE-$RUN_ID" --json)"
printf '%s\n' "$workspace" >"$OUT_DIR/workspace.json"
WORKSPACE_ID="$(printf '%s' "$workspace" | json_value workspace_id id)"

printf '%s\n' "$SHORT_TOKEN" >"$SHORT_PAYLOAD"
printf '%s\n' "$RECOVERY_TOKEN" >"$RECOVERY_PAYLOAD"
for size in $PAYLOAD_SIZES; do
  python3 "$BENCH/make_payload.py" "$OUT_DIR/payload-$size.txt" "$size" \
    "SIZE_${size}_${RUN_ID}"
done
[ -f "$LONG_PAYLOAD" ] || {
  python3 "$BENCH/make_payload.py" "$LONG_PAYLOAD" "$LONG_PAYLOAD_CHARS" "$LONG_TOKEN"
}

echo "run_id=$RUN_ID build=$build payload_sizes=$PAYLOAD_SIZES out=$OUT_DIR"
run_case short-mid-turn mid-turn "$SHORT_PAYLOAD" "$SHORT_TOKEN"
run_case "size-${LONG_PAYLOAD_CHARS}-idle" idle "$LONG_PAYLOAD" "$LONG_TOKEN"
for size in $PAYLOAD_SIZES; do
  run_case "size-$size-mid-turn" mid-turn "$OUT_DIR/payload-$size.txt" \
    "SIZE_${size}_${RUN_ID}"
done
run_recovery_case
echo "COMPLETE: observations are under $OUT_DIR"
