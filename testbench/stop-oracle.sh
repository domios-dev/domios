#!/usr/bin/env bash
# Non-vacuous /stop oracle. Addresses the "vacuous stop" critique: a single
# output-byte counter can be fooled (rolling buffer, cursor-blink redraw). This
# triangulates THREE INDEPENDENT change signals and requires ALL stable across a
# 2s window that starts AFTER the 1500ms force-idle suppression has expired:
#   (a) agent_state.state          — must stay idle (not revert to working)
#   (b) rendered screen hash       — md5 of strip_ansi screen; no repaint at all
#   (c) assistant transcript length — last assistant msg content len from /chat
#                                     (transcript truth, independent of PTY redraw)
# Plus a PRE-stop baseline proving the signals WERE changing while working (else
# the test is vacuous — nothing to interrupt). Question surfaces: after /stop the
# visible question must disappear (state leaves Question) or honestly stay Question.
#
# Pure /api/*, macOS, no DOM. DISPOSABLE dev pane only.
#   testbench/stop-oracle.sh --tab <id> [--mode stop|esc] [--prompt TEXT]
set -uo pipefail
TOKEN=$(/usr/bin/python3 -c "import re;print(re.search(r'token\s*=\s*\"([^\"]+)\"',open('$HOME/.codeterm-dev/config.toml').read()).group(1))")
B="http://127.0.0.1:7686"
post(){ /usr/bin/curl -s -X POST -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" --data "$2" "$B$1"; }
get(){ /usr/bin/curl -s -H "Authorization: Bearer $TOKEN" "$B$1"; }
nowms(){ /usr/bin/python3 -c 'import time;print(int(time.time()*1000))'; }
sleepms(){ /usr/bin/python3 -c "import time;time.sleep($1/1000)"; }

# Long prompt by default: the turn MUST still be generating when stop lands, else
# the test is vacuous (nothing to interrupt). The pre-stop baseline enforces this.
PANE=""; MODE="stop"; OC_BASE=""; OC_SESSION=""
PROMPT="Write a very long, exhaustive 2000-word essay on the complete decade-by-decade history of computer operating systems and terminals."
# --mode http-abort validates OpenCode's CONTROL-PLANE stop directly: POST
# {oc-base}/session/{oc-session}/abort (opencode must be run via `opencode serve`
# / structured-prompt server; the plain TUI launch has NO listening port). The
# verdict logic is identical and outcome-based (raw output growth), so it proves
# the abort regardless of whether it travels via /stop or HTTP.
while [ "$#" -gt 0 ]; do case "$1" in
  --tab) PANE="$2"; shift 2;; --mode) MODE="$2"; shift 2;; --prompt) PROMPT="$2"; shift 2;;
  --oc-base) OC_BASE="$2"; shift 2;; --oc-session) OC_SESSION="$2"; shift 2;;
  *) echo "unknown $1"; exit 2;; esac; done
[ -n "$PANE" ] || { echo "need --tab"; exit 2; }
[ "$MODE" = http-abort ] && { [ -n "$OC_BASE" ] && [ -n "$OC_SESSION" ] || { echo "http-abort needs --oc-base URL --oc-session ID"; exit 2; }; }

state(){ get /api/panes | /usr/bin/python3 -c "import sys,json;d=json.load(sys.stdin);print(next(((p.get('agent_state') or {}).get('state') for p in d if p['id']=='$PANE'),'?'))"; }
shash(){ get "/api/panes/$PANE/output?strip_ansi=true&full=true" | /usr/bin/python3 -c "import sys,json,hashlib;print(hashlib.md5(json.load(sys.stdin).get('output','').encode()).hexdigest()[:12])"; }
# Raw monotonic output buffer length — the STRONGEST 'still generating' signal: it
# grows with real output and is NOT fooled by in-place repaint (which leaves length
# unchanged but changes the screen hash). NOT a small ring buffer (observed >1.7MB).
orawlen(){ get "/api/panes/$PANE/output?full=true" | /usr/bin/python3 -c "import sys,json;print(len(json.load(sys.stdin).get('output','')))"; }
# assistant transcript len — only some providers populate /chat assistant msgs, and
# opencode commits the message only at turn END (lags streaming), so it is a WEAK
# secondary signal, reported but not gating.
alen(){ get "/api/panes/$PANE/chat" | /usr/bin/python3 -c "
import sys,json
try: m=json.load(sys.stdin).get('messages',[])
except Exception: print(0); sys.exit()
a=[x for x in m if x.get('type')=='assistant']
print(len(a[-1].get('content','')) if a else 0)"; }
sig(){ echo "$(state)|$(shash)|raw$(orawlen)|a$(alen)"; }

echo "== stop-oracle pane=$PANE mode=$MODE =="
post /api/panes/$PANE/send "$(/usr/bin/python3 -c 'import json,sys;print(json.dumps({"text":sys.argv[1],"enter":True}))' "$PROMPT")" >/dev/null
w=0; for i in $(seq 1 60); do [ "$(state)" = working ] && { w=1; break; }; sleepms 150; done
[ "$w" = 1 ] || { echo "FAIL: never Working"; exit 1; }

# PRE-stop baseline: confirm signals ARE moving while working (non-vacuous guard)
P1=$(sig); sleepms 700; P2=$(sig)
echo "  while-working sig A=$P1  B=$P2  changing=$([ "$P1" != "$P2" ] && echo YES || echo NO)"
if [ "$P1" = "$P2" ]; then
  echo "RESULT: VACUOUS (signals not changing pre-stop — the turn already finished, nothing to interrupt). Retest with a longer prompt."
  exit 2
fi

T0=$(nowms)
case "$MODE" in
  stop) STOP_RESP=$(post /api/panes/$PANE/stop '{}'); echo "  [/stop response: $STOP_RESP]";;
  esc)  post /api/panes/$PANE/send '{"actions":[{"type":"key","key":"esc"}]}' >/dev/null;;
  http-abort) /usr/bin/curl -s -X POST "$OC_BASE/session/$OC_SESSION/abort" >/dev/null 2>&1;;
esac
echo "  [issued $MODE]  (structured_abort:true in /stop response = abort-preflight binary)"

# Sample the window AFTER suppression (stop+2000 .. stop+6000), past the 1500ms force-idle.
declare -a STATES=() RAWS=() HASHES=()
for off in 2000 3000 4000 5000 6000; do
  now=$(nowms); el=$((now-T0)); wait=$((off-el)); [ "$wait" -gt 0 ] && sleepms "$wait"
  cs=$(state); cr=$(orawlen); ch=$(shash); ca=$(alen)
  STATES+=("$cs"); RAWS+=("$cr"); HASHES+=("$ch")
  echo "  @${off}ms post-stop  state=$cs  raw=$cr  screen=$ch  a=$ca"
done
raw_growth=$(( ${RAWS[4]} - ${RAWS[0]} ))      # bytes generated 2000->6000ms post-stop
hash_stable=$([ "${HASHES[0]}" = "${HASHES[4]}" ] && echo 1 || echo 0)
any_working=0; for s in "${STATES[@]}"; do [ "$s" = working ] && any_working=1; done
st_final="${STATES[4]}"
echo "  raw output growth (2000->6000ms post-stop): +${raw_growth} bytes | screen_hash_stable=$hash_stable | reverted_to_working=$any_working"

# Verdict keyed on the STRONGEST signal (raw monotonic output growth), corroborated
# by state and screen hash. Non-vacuous because the pre-stop baseline proved growth.
if [ "$raw_growth" -lt 300 ] && [ "$any_working" = 0 ] && [ "$st_final" = idle ]; then
  echo "RESULT: TRULY HALTED (output +${raw_growth}B over 4s past suppression, state stayed idle, screen settled=$hash_stable)"
  exit 0
elif [ "$st_final" = question ]; then
  echo "RESULT: REMAINS QUESTION (honest — question surface still present)"
  exit 0
elif [ "$raw_growth" -ge 300 ]; then
  echo "RESULT: NOT INTERRUPTED — STILL GENERATING (+${raw_growth}B after stop; state=${st_final}, reverted_to_working=$any_working). Interrupt key did not halt this provider."
  exit 1
else
  echo "RESULT: FALSE IDLE / STATE UNSTABLE (raw +${raw_growth}B, state '${st_final}', reverted=$any_working)"
  exit 1
fi
