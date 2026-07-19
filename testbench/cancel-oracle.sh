#!/usr/bin/env bash
# Stop / Cancel / manual-Esc -> Idle acceptance oracle. Pure /api/* — runs on
# macOS, no WebView2/DOM. Drives a (disposable) agent pane to Working, issues the
# stop action, and asserts agent_state.state flips to idle within a timeout.
#
# Mechanism under test:
#   Stop button   : POST /api/panes/{id}/stop          (writes provider stop_keystroke; ESC for claude/codex/opencode)
#   manual Esc    : POST /api/panes/{id}/send {actions:[{key:esc}]}  (keys.rs: esc -> \x1b)
#   Cancel prompt : POST /api/panes/{id}/prompt/clear  (clears a pending question)
#
# SAFETY: never targets Andrey's working panes. Pass a disposable pane id, or use
# --wiring to only verify endpoints answer on an idle pane (no agent disruption).
#
# Usage:
#   testbench/cancel-oracle.sh --tab <id> --mode stop|esc|cancel [--prompt TEXT] [--timeout-ms 8000]
#   testbench/cancel-oracle.sh --wiring --tab <id>      # endpoint smoke only, no drive
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"; cd "$ROOT"
source evals/_lib/lib.sh 2>/dev/null

PANE=""; MODE="stop"; PROMPT="count slowly from 1 to 40, one number per line, pausing between each"; TIMEOUT_MS=8000; WIRING=0
while [ "$#" -gt 0 ]; do case "$1" in
  --tab) PANE="$2"; shift 2;; --mode) MODE="$2"; shift 2;;
  --prompt) PROMPT="$2"; shift 2;; --timeout-ms) TIMEOUT_MS="$2"; shift 2;;
  --wiring) WIRING=1; shift;; *) echo "unknown $1"; exit 2;; esac; done
[ -n "$PANE" ] || { echo "need --tab <id>"; exit 2; }

state() { dev_get /api/panes 2>/dev/null | python3 -c "
import sys,json
d=json.load(sys.stdin)
for p in d:
    if p['id']=='$PANE':
        s=(p.get('agent_state') or {}).get('state'); print(s if s else 'null'); break
else: print('nopane')
"; }

issue_stop() {
  case "$MODE" in
    stop)   dev_post "/api/panes/$PANE/stop" '{}' >/dev/null 2>&1;;
    esc)    dev_post "/api/panes/$PANE/send" '{"actions":[{"type":"key","key":"esc"}]}' >/dev/null 2>&1;;
    cancel) dev_post "/api/panes/$PANE/prompt/clear" '{}' >/dev/null 2>&1;;
  esac
}

if [ "$WIRING" = 1 ]; then
  echo "[wiring] pane $PANE current state: $(state)"
  echo "[wiring] issuing mode=$MODE (idempotent on idle pane)"
  issue_stop
  echo "[wiring] post-issue state: $(state)  (endpoints reachable if no error above)"
  exit 0
fi

echo "== cancel-oracle pane=$PANE mode=$MODE timeout=${TIMEOUT_MS}ms =="
echo "[1] baseline state: $(state)"
echo "[2] driving to Working: $PROMPT"
dev_post "/api/panes/$PANE/send" "$(python3 -c 'import json,sys;print(json.dumps({"text":sys.argv[1],"enter":True}))' "$PROMPT")" >/dev/null 2>&1

# wait up to 6s for Working
work=0
for i in $(seq 1 60); do
  s="$(state)"; [ "$s" = "working" ] && { work=1; echo "[3] reached Working after ~$((i*100))ms"; break; }
  python3 -c 'import time;time.sleep(0.1)'
done
[ "$work" = 1 ] || { echo "[3] FAIL: never reached Working (state=$(state)) — cannot test stop"; exit 1; }

echo "[4] issuing $MODE"
START=$(python3 -c 'import time;print(int(time.time()*1000))')
issue_stop

idle=0
while :; do
  s="$(state)"
  NOW=$(python3 -c 'import time;print(int(time.time()*1000))')
  EL=$((NOW-START))
  if [ "$s" = "idle" ]; then idle=1; echo "[5] reached Idle after ${EL}ms"; break; fi
  [ "$EL" -ge "$TIMEOUT_MS" ] && { echo "[5] FAIL: still '$s' after ${EL}ms (timeout)"; break; }
  python3 -c 'import time;time.sleep(0.1)'
done

echo "== RESULT: $([ "$idle" = 1 ] && echo PASS || echo FAIL) (mode=$MODE) =="
exit $([ "$idle" = 1 ] && echo 0 || echo 1)
