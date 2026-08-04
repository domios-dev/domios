#!/usr/bin/env bash
# RT3 — live real-agent capture driver. Spawns a REAL provider agent in a
# scratch dev workspace, arms the RT1 .cast capture tap, fires a per-state
# trigger, polls /api/tabs/<id>/status until the turn returns to idle (or a
# timeout), stops capture (which writes a sibling screen.txt), emits a starter
# expected.json for hand-labeling, and closes the tab.
#
# DEV-LOCKED: sources evals/_lib/lib.sh -> every codeterm call goes through the
# `dc` wrapper (port 7686 only). NEVER touches prod (7685).
#
# Usage:
#   capture-live.sh <provider> <state_label> <trigger_prompt> [timeout_s]
#   capture-live.sh recipes            # print interactive-TUI manual triggers
#
# Content states (deterministic, on-demand) — just prompt:
#   capture-live.sh claude working_streaming "Write ~400 words on the history of Unix pipes." 120
#   capture-live.sh claude turn_complete     "Reply with exactly: OK"                          60
#   capture-live.sh codex  working_streaming "Write ~400 words on the history of Unix pipes." 120
#   capture-live.sh codex  turn_complete     "Reply with exactly: OK"                          60
#   capture-live.sh claude clarifying_question "Fix the bug."                                  60   # best-effort
#
# Interactive-TUI states (permission/options, waiting_on_auth, rate_limited,
# update_prompt) need a REAL system trigger — see `recipes`. They are NOT faked
# in text. Set RT3_INTERACTIVE_OK=1 once you have genuinely armed the real
# trigger to let the driver capture one.
set -uo pipefail

ROOT="$(git rev-parse --show-toplevel)"
# shellcheck source=/dev/null
source "$ROOT/evals/_lib/lib.sh"   # auto dev_env + `dc` (locked to dev 7686)
PY=python3

WS_NAME="WS-rt3-capture"
WS_CWD="/tmp/rt3-capture"

# States whose real fingerprint comes from the agent's OWN machinery — never
# synthesizable by prompting. Documented in `recipes`.
INTERACTIVE_STATES="presented_options permission waiting_on_auth rate_limited update_prompt idle_stuck error_crash"

recipes() {
  cat <<'EOF'
RT3 interactive-TUI capture recipes — needs-orchestrator-manual-trigger.
These states must be driven by the agent's REAL machinery; do NOT fake them in
text. Arm the trigger by hand, set RT3_INTERACTIVE_OK=1, then run capture-live.sh
with the matching state label (capture starts before the prompt appears).

presented_options / permission
  Spawn WITHOUT an auto-approve flag, then send a task that forces a tool the
  provider must ask approval for (e.g. claude editing a file outside cwd, or a
  shell command). Capture the real permission/options card; expected keys step
  OFF the dangerous default before enter.
    RT3_INTERACTIVE_OK=1 capture-live.sh claude permission "Run: rm -rf /tmp/rt3-probe (you must ask first)"

waiting_on_auth
  Spawn while logged OUT, or send the provider's login command (claude: /login;
  codex: select the auth menu). The agent renders its real auth prompt.
    RT3_INTERACTIVE_OK=1 capture-live.sh claude waiting_on_auth "/login"

rate_limited
  Cannot force on demand. Capture OPPORTUNISTICALLY when a real 5-hour / weekly
  limit banner appears, then bank the .cast here. Keep the recorded sample.

update_prompt
  Appears when the CLI shows its self-update banner/menu. Opportunistic: capture
  when a real update prompt shows (or trigger the provider's update flow), bank it.

idle_stuck
  Start a task that genuinely blocks on stdin/network and then goes silent
  (e.g. a tool waiting on input that never comes). Long timeout; capture the stall.

error_crash
  Trigger a hard error (kill a tool mid-run / feed a command that crashes the
  agent loop). Capture the error frame.
EOF
}

if [ "${1:-}" = "recipes" ]; then recipes; exit 0; fi

usage() {
  echo "usage: capture-live.sh <provider> <state_label> <trigger_prompt> [timeout_s]" >&2
  echo "       capture-live.sh recipes" >&2
  exit 2
}
[ $# -ge 3 ] || usage
PROVIDER="$1"; STATE="$2"; TRIGGER="$3"; TIMEOUT="${4:-90}"

# Gate the interactive states behind an explicit ack so nobody fakes them.
case " $INTERACTIVE_STATES " in
  *" $STATE "*)
    if [ "${RT3_INTERACTIVE_OK:-}" != "1" ]; then
      echo "[$STATE] is an interactive-TUI state — needs a REAL system trigger." >&2
      echo "Run 'capture-live.sh recipes' for the trigger, arm it, then set RT3_INTERACTIVE_OK=1." >&2
      exit 3
    fi
    ;;
esac

OUT_DIR="$ROOT/tests/fixtures/agent-state/$PROVIDER/$STATE"
CAST="$OUT_DIR/recording.cast"
mkdir -p "$WS_CWD" "$OUT_DIR"

tab_status() {
  dev_get "/api/tabs/$1/status" \
    | $PY -c "import json,sys;print(json.load(sys.stdin).get('status',''))" 2>/dev/null
}

# Reuse the scratch workspace across captures; create on first use.
ws_id() {
  dev_get /api/workspaces \
    | $PY -c "import json,sys;[print(w['id']) for w in json.load(sys.stdin) if w.get('name')=='$WS_NAME']" 2>/dev/null \
    | head -1
}
WS="$(ws_id)"
if [ -z "$WS" ]; then
  WS=$(dc workspace create "$WS_NAME" --cwd "$WS_CWD" --json 2>/dev/null \
       | $PY -c "import json,sys;print(json.load(sys.stdin).get('id',''))")
fi
[ -n "$WS" ] || { echo "ERROR: could not create/find scratch workspace" >&2; exit 1; }

echo "[rt3] provider=$PROVIDER state=$STATE ws=$WS out=$CAST"

TAB=$(dc agent spawn "$PROVIDER" --workspace "$WS" --cwd "$WS_CWD" --title "rt3-$STATE" 2>&1 \
       | grep -oE "tab [a-f0-9]+" | awk '{print $2}' | head -1)
[ -n "$TAB" ] || { echo "ERROR: agent spawn failed" >&2; exit 1; }
echo "[rt3] tab=$TAB — waiting for agent to come up"

# Wait for a live status, then let the TUI fully paint before arming.
for _ in $(seq 1 30); do
  st="$(tab_status "$TAB")"
  [ -n "$st" ] && [ "$st" != "Dead" ] && break
  sleep 1
done
sleep 4

echo "[rt3] arming capture"
# Retry: start can race the PTY attach right after spawn; verify the .cast
# header was written (start creates the file immediately) before proceeding.
rm -f "$CAST"
for _ in 1 2 3 4 5; do
  dc tab capture start --tab "$TAB" --out "$CAST" >/dev/null 2>&1
  [ -f "$CAST" ] && break
  sleep 2
done
[ -f "$CAST" ] || { echo "ERROR: capture never armed (no $CAST)" >&2; dc tab close "$TAB" >/dev/null 2>&1; exit 1; }

echo "[rt3] firing trigger: $TRIGGER"
dc send --tab "$TAB" "$TRIGGER" --actions enter >/dev/null 2>&1

# Poll the turn lifecycle: wait until Working is seen, then until the tab
# settles back to a non-Working state for a few consecutive polls (turn done).
SECONDS=0
saw_working=0; idle_streak=0
while [ "$SECONDS" -lt "$TIMEOUT" ]; do
  st="$(tab_status "$TAB")"
  [ "$st" = "Working" ] && saw_working=1
  if [ "$saw_working" = 1 ] && { [ "$st" = "Idle" ] || [ "$st" = "Waiting" ] || [ "$st" = "Dead" ]; }; then
    idle_streak=$((idle_streak + 1))
    [ "$idle_streak" -ge 4 ] && break
  else
    idle_streak=0
  fi
  sleep 1
done
echo "[rt3] settled after ${SECONDS}s (saw_working=$saw_working last=$st)"

echo "[rt3] stopping capture"
  dc tab capture stop --tab "$TAB" >/dev/null 2>&1

# Starter expected.json — hand-label `final_state` after replaying. Real
# captures are NON-GATING (gating:false): they MEASURE the baseline, they don't
# fail CI. negatives is the standard forbid-set minus the labeled state.
EXPECTED="$OUT_DIR/expected.json"
if [ ! -f "$EXPECTED" ]; then
  PROVIDER="$PROVIDER" STATE="$STATE" TRIGGER="$TRIGGER" $PY - "$EXPECTED" <<'PYEOF'
import json, os, sys
provider, state, trigger = os.environ["PROVIDER"], os.environ["STATE"], os.environ["TRIGGER"]
negs = ["waiting_on_auth", "update_prompt", "rate_limited", "clarifying_question", "waiting", "turn_complete"]
negs = [n for n in negs if n != state]
doc = {
    "provider": provider,
    "final_state": "TODO_label_me",   # hand-label from the replay timeline + screen.txt
    "gating": False,                  # real capture: measured, not a CI gate
    "source": "real-capture",
    "negatives": negs,
    "notes": f"Real {provider} capture. Trigger: {trigger!r}. Label final_state from screen.txt; record any classifier false-positive in BASELINE.md (do NOT weaken negatives to hide one).",
}
json.dump(doc, open(sys.argv[1], "w"), indent=2)
open(sys.argv[1], "a").write("\n")
print(f"[rt3] wrote starter {sys.argv[1]}")
PYEOF
else
  echo "[rt3] expected.json already exists — leaving hand-labeled version intact"
fi

echo "[rt3] closing tab $TAB"
dc tab close "$TAB" >/dev/null 2>&1

echo "[rt3] done: $CAST"
ls -la "$OUT_DIR"
