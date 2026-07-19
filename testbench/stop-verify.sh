#!/usr/bin/env bash
# /stop verification — distinguishes "classifier flipped to Idle" from "agent was
# ACTUALLY interrupted". /stop sends ESC + mark_agent_interrupted (force-idle +
# AGENT_STOP_SUPPRESS_MS=1500ms suppression). During the window the state is FORCED
# idle regardless of reality; the real proof is what happens AFTER it expires:
#   * actually interrupted : state stays idle past 1500ms AND PTY output stops growing.
#   * false idle (illusion): state reverts to working after ~1500ms, OR output keeps
#     growing (ESC didn't halt the agent — the "1500ms illusion" the code warns about).
#
# Pure /api/*, macOS, no DOM. Use a DISPOSABLE dev pane.
#   testbench/stop-verify.sh --tab <id> [--mode stop|esc] [--prompt TEXT]
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"; cd "$ROOT"
TOKEN=$(/usr/bin/python3 -c "import re;print(re.search(r'token\s*=\s*\"([^\"]+)\"',open('$HOME/.codeterm-dev/config.toml').read()).group(1))")
B="http://127.0.0.1:7686"
post(){ /usr/bin/curl -s -X POST -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" --data "$2" "$B$1"; }
get(){ /usr/bin/curl -s -H "Authorization: Bearer $TOKEN" "$B$1"; }
nowms(){ /usr/bin/python3 -c 'import time;print(int(time.time()*1000))'; }
sleepms(){ /usr/bin/python3 -c "import time;time.sleep($1/1000)"; }

PANE=""; MODE="stop"; PROMPT="Write a 600-word essay about the history of computer terminals, in full detail."
while [ "$#" -gt 0 ]; do case "$1" in --tab) PANE="$2"; shift 2;; --mode) MODE="$2"; shift 2;; --prompt) PROMPT="$2"; shift 2;; *) echo "unknown $1"; exit 2;; esac; done
[ -n "$PANE" ] || { echo "need --tab <id>"; exit 2; }

state(){ get /api/panes | /usr/bin/python3 -c "import sys,json;d=json.load(sys.stdin);print(next(((p.get('agent_state') or {}).get('state') for p in d if p['id']=='$PANE'),'?'))"; }
outlen(){ get "/api/panes/$PANE/output?full=true" | /usr/bin/python3 -c "import sys,json;print(len(json.load(sys.stdin).get('output','')))"; }

echo "== stop-verify pane=$PANE mode=$MODE =="
post /api/panes/$PANE/send "$(/usr/bin/python3 -c 'import json,sys;print(json.dumps({"text":sys.argv[1],"enter":True}))' "$PROMPT")" >/dev/null
w=0; for i in $(seq 1 60); do [ "$(state)" = working ] && { w=1; break; }; sleepms 150; done
[ "$w" = 1 ] || { echo "FAIL: never reached Working (state=$(state))"; exit 1; }
echo "[reached Working]"

L0=$(outlen)
T0=$(nowms)
case "$MODE" in stop) post /api/panes/$PANE/stop '{}' >/dev/null;; esc) post /api/panes/$PANE/send '{"actions":[{"type":"key","key":"esc"}]}' >/dev/null;; esac
echo "[issued $MODE @ T0, output_len=$L0]  (suppression window = 1500ms)"

# Sample state + cumulative output growth at fixed offsets. The discriminator is
# the LATE window (3500->6000ms): if output is still growing AFTER suppression has
# long expired, generation did NOT halt (false idle). Early growth is expected
# (in-flight bytes + interrupt redraw + 'Interrupted' text).
declare -a TS=(500 1500 3500 6000)
declare -a SV=(); declare -a LV=()
for off in "${TS[@]}"; do
  now=$(nowms); el=$((now-T0)); wait=$((off-el)); [ "$wait" -gt 0 ] && sleepms "$wait"
  s=$(state); L=$(outlen)
  SV+=("$s"); LV+=("$L")
  printf "  @%5dms  state=%-8s  cum_out_delta=+%d\n" "$off" "$s" "$((L-L0))" >&2
done
late_growth=$(( ${LV[3]} - ${LV[2]} ))   # bytes added between 3500ms and 6000ms
early_post=$(( ${LV[2]} - ${LV[1]} ))    # bytes added 1500ms->3500ms (just after suppression)
s_500=${SV[0]}; s_post=${SV[2]}; s_final=${SV[3]}
halted=$([ "$late_growth" -lt 80 ] && [ "$early_post" -lt 80 ] && echo 1 || echo 0)
echo "  growth post-suppress(1500->3500ms): +${early_post}B | late(3500->6000ms): +${late_growth}B"
# Two independent axes: (A) did GENERATION halt? (B) did STATE settle idle, and how fast?
if [ "$halted" = 1 ] && [ "$s_post" = idle ]; then
  echo "RESULT: CLEAN INTERRUPT (generation halted AND state idle by 3500ms)"
  exit 0
elif [ "$halted" = 1 ] && [ "$s_final" = idle ]; then
  echo "RESULT: HALTED, STATE LAGGED (generation halted immediately but state stayed '$s_post' until ~6000ms — classifier idle-latency gap, not a generation gap)"
  exit 0
elif [ "$halted" = 1 ]; then
  echo "RESULT: HALTED, STATE STILL '$s_final' (generation halted but classifier never settled idle within 6s)"
  exit 1
elif [ "$s_final" != idle ]; then
  echo "RESULT: NOT INTERRUPTED (state '$s_final' and generation continuing: +${late_growth}B late)"
  exit 1
else
  echo "RESULT: FALSE IDLE (state idle but generation continuing: +${late_growth}B late — the 1500ms illusion)"
  exit 1
fi
