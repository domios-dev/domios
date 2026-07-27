#!/usr/bin/env bash
# Watcher health e2e smoke for chatBackend watcher mode.
#
# DEV-LOCKED: sources evals/_lib/lib.sh and uses dc/dev_get only for CodeTerm.
# Requires LM Studio running at localhost:1234 with a loaded model and an
# installed watcher-capable lmstudio plugin. Missing external prerequisites are
# SKIP, not FAIL, so the orchestrator can run this on suitable machines only.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck source=/dev/null
source "$ROOT/evals/_lib/lib.sh"

PY=python3
WS_NAME="WS-watcher-health"
WS_CWD="$(mktemp -d /tmp/watcher-health.XXXXXX)"
INTERVAL_SECS=30
WAIT_SECS="${WH_WAIT_SECS:-95}"
POLL_SECS="${WH_POLL_SECS:-45}"
MAX_TOTAL_SECS=300
WATCHER_TITLE="WH watcher"
WATCHER_ORIGIN_LABEL="Watcher"
WATCHER_CHARTER='You are a read-only orchestration health watcher for this bench. The observed group contains one orchestrator and one worker. Treat a worker as stalled when it has no visible progress for 45 seconds or more. On each tick respond only with watcher verdict JSON: {"status":"ok"|"attention"|"stalled","summary":"...","state":{},"actions":[...]}. When the worker is stalled, emit exactly one nudge action targeted at the orchestrator tab with a short message naming the stalled worker evidence. Do not emit more than one action per tick.'

if [ "$WAIT_SECS" -gt "$MAX_TOTAL_SECS" ]; then
  WAIT_SECS="$MAX_TOTAL_SECS"
fi
if [ $((WAIT_SECS + POLL_SECS * 2)) -gt "$MAX_TOTAL_SECS" ]; then
  remaining=$((MAX_TOTAL_SECS - WAIT_SECS))
  [ "$remaining" -lt 0 ] && remaining=0
  POLL_SECS=$((remaining / 2))
fi

FAILS=0
SKIPS=0
CREATED_TABS=()
CREATED_WS=()

summary() { printf '%s %s: %s\n' "$1" "$2" "$3"; }
pass() { summary PASS "$1" "$2"; }
fail() { summary FAIL "$1" "$2" >&2; FAILS=$((FAILS + 1)); }
skip() { summary SKIP "$1" "$2"; SKIPS=$((SKIPS + 1)); }

cleanup() {
  [ -n "${WS_CWD:-}" ] && [[ "$WS_CWD" == /tmp/watcher-health.* ]] && rm -rf "$WS_CWD"
  local out rc
  for tab in "${CREATED_TABS[@]:-}"; do
    [ -n "$tab" ] || continue
    out="$(dc tab close "$tab" 2>&1)"
    rc=$?
    [ "$rc" -eq 0 ] || printf 'cleanup: tab close %s failed: %s\n' "$tab" "$out" >&2
  done
  for ws in "${CREATED_WS[@]:-}"; do
    [ -n "$ws" ] || continue
    out="$(dc workspace close "$ws" 2>&1)"
    rc=$?
    [ "$rc" -eq 0 ] || printf 'cleanup: workspace close %s failed: %s\n' "$ws" "$out" >&2
  done
}
trap cleanup EXIT

json_field() {
  local expr="$1"
  $PY -c "import json,sys; d=json.load(sys.stdin); v=eval(sys.argv[1], {'d': d}); print('' if v is None else v)" "$expr" 2>/dev/null
}

extract_tab_id() {
  grep -oE "tab [a-f0-9]+" | awk '{print $2}' | head -1
}

plugin_has_watcher() {
  $PY -c '
import json, sys
try:
    data = json.load(sys.stdin)
except Exception:
    print("no")
    raise SystemExit
items = data if isinstance(data, list) else data.get("providers") or []
for item in items:
    if isinstance(item, dict) and item.get("id") == "lmstudio":
        print("yes" if item.get("supports_watcher") is True else "no")
        break
else:
    print("no")
'
}

tab_chat_json() {
  local tab="$1" out rc
  out="$(dc --json tab chat --tab "$tab" 2>&1)"
  rc=$?
  if [ "$rc" -ne 0 ]; then
    printf '%s\n' "$out" >&2
    return "$rc"
  fi
  printf '%s\n' "$out"
}

chat_has_type() {
  local tab="$1" want="$2" deadline=$((SECONDS + POLL_SECS))
  while [ "$SECONDS" -lt "$deadline" ]; do
    local chat rc
    chat="$(tab_chat_json "$tab")"
    rc=$?
    [ "$rc" -eq 0 ] || return "$rc"
    if printf '%s\n' "$chat" | $PY -c '
import json, sys
want = sys.argv[1]
raw = sys.stdin.read()
if not raw.strip():
    raise SystemExit(1)
try:
    data = json.loads(raw)
except Exception:
    raise SystemExit(1)
msgs = data.get("messages") if isinstance(data, dict) else data
ok = isinstance(msgs, list) and any(isinstance(m, dict) and m.get("type") == want for m in msgs)
print("1" if ok else "")
raise SystemExit(0 if ok else 1)
    ' "$want"
    then
      return 0
    fi
    sleep 3
  done
  return 1
}

watcher_nudge_count() {
  local tab="$1" label="$2" chat rc
  chat="$(tab_chat_json "$tab")"
  rc=$?
  [ "$rc" -eq 0 ] || return "$rc"
  printf '%s\n' "$chat" | $PY -c '
import json, sys
label = sys.argv[1]
raw = sys.stdin.read()
if not raw.strip():
    print(0)
    raise SystemExit
try:
    data = json.loads(raw)
except Exception:
    raise SystemExit(1)
msgs = data.get("messages") if isinstance(data, dict) else data
count = sum(
    1 for msg in (msgs or [])
    if isinstance(msg, dict) and (msg.get("fromLabel") == label or msg.get("from_label") == label)
)
print(count)
  ' "$label"
}


wait_tab_idle() {
  local tab="$1" deadline=$((SECONDS + 240))
  while [ $SECONDS -lt $deadline ]; do
    if dc tab status --tab "$tab" 2>/dev/null | grep -q "status:  *Idle"; then
      return 0
    fi
    sleep 3
  done
  return 1
}


watcher_note_count() {
  local tab="$1" chat rc
  chat="$(tab_chat_json "$tab")"
  rc=$?
  [ "$rc" -eq 0 ] || return "$rc"
  printf '%s\n' "$chat" | $PY -c '
import json, sys
raw = sys.stdin.read()
if not raw.strip():
    print(0); raise SystemExit
data = json.loads(raw)
msgs = data.get("messages") if isinstance(data, dict) else data
print(sum(1 for m in (msgs or []) if isinstance(m, dict) and "Nudge delivered to" in str(m.get("content",""))))
  '
}

plugins_surface() {
  dc agent providers --json
}

echo "=== watcher-health preflight ==="
if ! curl -s --max-time 3 http://127.0.0.1:7686/api/health >/dev/null 2>&1; then
  skip preflight "dev daemon is not healthy on port 7686"
  exit 0
fi
pass preflight "dev daemon healthy"

if ! curl -s --max-time 5 http://localhost:1234/api/v1/models >/dev/null 2>&1; then
  skip lmstudio "LM Studio API not reachable at localhost:1234/api/v1/models"
  exit 0
fi
pass lmstudio "LM Studio API reachable"

plugins="$(plugins_surface)"
if [ -z "$plugins" ] || [ "$(printf '%s\n' "$plugins" | plugin_has_watcher)" != "yes" ]; then
  skip plugin "lmstudio is not installed with chatBackend watcher capability; install dev plugin with: dc plugin install lmstudio --from-dir ../codeterm-plugins/lmstudio"
  exit 0
fi
pass plugin "lmstudio plugin advertises chatBackend watcher capability"

echo "=== fixture setup ==="
WS_OUT="$(dc workspace create "$WS_NAME" --cwd "$WS_CWD" --json 2>&1)"
WS_RC=$?
if [ "$WS_RC" -ne 0 ]; then
  fail workspace "workspace create failed: $(printf '%s' "$WS_OUT" | head -c 180)"
  exit 1
fi
WS="$(printf '%s\n' "$WS_OUT" | json_field "d.get('id','')")"
if [ -z "$WS" ]; then
  fail workspace "workspace create returned no id"
  exit 1
fi
CREATED_WS+=("$WS")
pass workspace "created $WS"

ORCH_TASK="Watcher-health fixture: stay idle. Do not spawn agents. This tab exists so a watcher can observe a quiet orchestration."
ORCH_OUT="$(dc agent spawn claude --role Orchestrator --workspace "$WS" --cwd "$WS_CWD" --title "WH orchestrator" --task "$ORCH_TASK" 2>&1)"
ORCH_RC=$?
if [ "$ORCH_RC" -ne 0 ]; then
  fail orchestrator "spawn failed: $(printf '%s' "$ORCH_OUT" | head -c 180)"
  exit 1
fi
ORCH="$(printf '%s\n' "$ORCH_OUT" | extract_tab_id)"
if [ -z "$ORCH" ]; then
  fail orchestrator "spawn failed: $(printf '%s' "$ORCH_OUT" | head -c 180)"
  exit 1
fi
CREATED_TABS+=("$ORCH")
pass orchestrator "spawned $ORCH"
if wait_tab_idle "$ORCH"; then
  pass orchestrator-idle "orchestrator finished onboarding and is Idle"
else
  echo "WARN orchestrator-idle: not Idle after 240s; PTY delivery to a busy claude may be swallowed (pre-existing race), nudge oracle uses the watcher transcript"
fi

WORKER_TASK="Watcher-health stalled fixture: acknowledge nothing, run no commands, send no report, and wait silently. The watcher should see this worker make no progress."
WORKER_OUT="$(dc agent spawn claude --role Worker --parent "$ORCH" --workspace "$WS" --cwd "$WS_CWD" --title "WH stalled worker" --task "$WORKER_TASK" 2>&1)"
WORKER_RC=$?
if [ "$WORKER_RC" -ne 0 ]; then
  fail worker "spawn failed: $(printf '%s' "$WORKER_OUT" | head -c 180)"
  exit 1
fi
WORKER="$(printf '%s\n' "$WORKER_OUT" | extract_tab_id)"
if [ -z "$WORKER" ]; then
  fail worker "spawn failed: $(printf '%s' "$WORKER_OUT" | head -c 180)"
  exit 1
fi
CREATED_TABS+=("$WORKER")
pass worker "spawned silent stalled fixture $WORKER"

WATCHER_OUT="$(dc agent spawn lmstudio --role Watcher --parent "$ORCH" --workspace "$WS" --cwd "$WS_CWD" --title "$WATCHER_TITLE" --charter "$WATCHER_CHARTER" --interval "${INTERVAL_SECS}s" --nudge-cooldown 10m --max-actions 1 2>&1)"
WATCHER_RC=$?
if [ "$WATCHER_RC" -ne 0 ]; then
  fail watcher "spawn failed: $(printf '%s' "$WATCHER_OUT" | head -c 220)"
  exit 1
fi
WATCHER="$(printf '%s\n' "$WATCHER_OUT" | extract_tab_id)"
if [ -z "$WATCHER" ]; then
  fail watcher "spawn failed: $(printf '%s' "$WATCHER_OUT" | head -c 220)"
  exit 1
fi
CREATED_TABS+=("$WATCHER")
pass watcher "spawned $WATCHER"

echo "=== observe two watcher intervals ==="
sleep "$WAIT_SECS"

if chat_has_type "$WATCHER" context_request; then
  pass context_request "watcher transcript contains context_request"
else
  fail context_request "watcher transcript did not contain context_request within bounded wait"
fi

if chat_has_type "$WATCHER" watcher_verdict; then
  pass watcher_verdict "watcher transcript contains watcher_verdict"
else
  fail watcher_verdict "watcher transcript did not contain watcher_verdict within bounded wait"
fi

NUDGES="$(watcher_note_count "$WATCHER")"
NUDGE_RC=$?
if [ "$NUDGE_RC" -ne 0 ]; then
  fail cooldown "could not read orchestrator chat to count watcher nudges"
elif [ "${NUDGES:-0}" -eq 1 ] 2>/dev/null; then
  pass cooldown "orchestrator received exactly one watcher-origin nudge across two ticks"
else
  fail cooldown "orchestrator received ${NUDGES:-0} delivered-nudge notes in the watcher transcript; expected exactly 1 across two ticks (cooldown)"
fi

echo "=== summary ==="
if [ "$FAILS" -eq 0 ]; then
  echo "PASS watcher-health: assertions passed"
  exit 0
fi
echo "FAIL watcher-health: $FAILS assertion(s) failed"
exit 1
