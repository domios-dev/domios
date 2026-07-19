#!/usr/bin/env bash
# Ordered-transcript drill: the PROOF that `codeterm orchestrator refresh` runs
# S1..S5 in strict order. Asserts:
#   handoff-stride ts  <  compact cmd  <  compaction signature settle
#     <  re-orient injection  <  first post-refresh plan-state read (resumed)
# and that post-refresh current_focus still reflects the pre-refresh frontier.
#
# Provider is selectable: PROVIDER=claude (default) | codex. Codex usage may be
# exhausted — the orchestrator schedules the codex run when headroom returns.
#
# Usage: PROVIDER=claude ./evals/orchestrator-refresh/drill.sh
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
source "$HERE/lib.sh"

dev_alive || { echo "ERROR: dev not running on 7686. ./build.sh dev" >&2; exit 1; }

PROVIDER="${PROVIDER:-claude}"
CWD="$(pwd)/target-check/evals/orchestrator-refresh"
mkdir -p "$CWD"

# Realistic timeouts for a LIVE agent: /compact alone takes ~40s; a fresh agent
# needs seconds to read+run each injected command. Tight caps strand the cycle.
dc config set orchestration.refresh.handoff_timeout_secs 60 >/dev/null 2>&1
dc config set orchestration.refresh.settle_timeout_secs 40 >/dev/null 2>&1
dc config set orchestration.refresh.verify_timeout_secs 90 >/dev/null 2>&1

echo "== [1/6] spawn a $PROVIDER orchestrator pane + bind a plan"
WS=$(new_workspace "REFRESH-DRILL" "$CWD")
[ -n "$WS" ] || { echo "FAIL: workspace create" >&2; exit 1; }

case "$PROVIDER" in
  claude) PANE=$(new_claude_pane "$WS" "$CWD" "orch") ;;
  codex)  PANE=$(new_codex_pane  "$WS" "$CWD" "orch") ;;
  *) echo "FAIL: unknown PROVIDER=$PROVIDER" >&2; exit 1 ;;
esac
[ -n "$PANE" ] || { echo "FAIL: pane spawn ($PROVIDER)" >&2; exit 1; }
echo "   pane=$PANE"

PLAN=$(dc plan create --title "refresh drill" --json 2>/dev/null \
  | python -c "import json,sys; print(json.load(sys.stdin).get('plan_id',''))")
[ -n "$PLAN" ] || { echo "FAIL: plan create" >&2; exit 1; }
echo "   plan=$PLAN"

# Establish a pre-refresh frontier the orchestrator is meant to preserve.
dc orchestrator heartbeat set --plan "$PLAN" --focus "ship refresh cycle" >/dev/null 2>&1
FRONTIER_BEFORE=$(plan_focus "$PLAN")
echo "   frontier_before='$FRONTIER_BEFORE'"

echo "== [2/6] wait for the pane to reach READY, then prime its orchestrator contract"
# Real readiness signal (learned by manual walk): the daemon logs 'agent ready in
# pane X' ~14s after spawn. A fixed sleep races the boot and the S1 injection lands
# in a not-yet-ready agent that never submits it.
for _ in $(seq 1 30); do
  grep -qaE "agent ready in pane $PANE" "$DEV_LOG" 2>/dev/null && break
  sleep 2
done
sleep 3
# A BARE agent strands the cycle — prime it to run the exact command each injection names.
dc send --tab "$PANE" "You are a test orchestrator. Stay idle at your prompt. When you receive an injected message naming an exact shell command (e.g. codeterm orchestrator handoff --plan ..., or codeterm plan state ...), run THAT exact command verbatim, using the plan id written in the message. Do nothing else." >/dev/null 2>&1
sleep 2

OFF=$(log_offset)

echo "== [3/6] trigger refresh"
dc orchestrator refresh --plan "$PLAN" --tab "$PANE" || {
  echo "FAIL: refresh trigger returned non-zero" >&2; exit 1; }

echo "== [4/6] wait for the terminal marker (resumed or alert)"
if ! wait_for_marker "$OFF" "[refresh] pane=" 30; then
  echo "FAIL: no refresh markers emitted at all" >&2; exit 1
fi
# Give the ordered cycle time to walk S1..S5 (bounded by settle+verify defaults).
wait_for_marker "$OFF" "resumed" 320 || echo "WARN: no 'resumed' — inspect failure branch"

TAILTEXT="$(log_since "$OFF")"

echo "== [5/6] assert strict transcript ORDER"
rc=0
assert_order "$TAILTEXT" "phase=s1_handoff" "compact mode="   || rc=1
assert_order "$TAILTEXT" "compact mode="    "phase=s3_settle" || rc=1
assert_order "$TAILTEXT" "phase=s3_settle"  "phase=s4_reorient" || rc=1
assert_order "$TAILTEXT" "phase=s4_reorient" "phase=s5_verify" || rc=1
assert_order "$TAILTEXT" "phase=s5_verify"  "resumed"         || rc=1

echo "== [6/6] assert continuity: post-refresh frontier preserved"
FRONTIER_AFTER=$(plan_focus "$PLAN")
echo "   frontier_after='$FRONTIER_AFTER'"
if [ "$FRONTIER_BEFORE" != "$FRONTIER_AFTER" ]; then
  echo "FAIL: current_focus drifted across refresh ('$FRONTIER_BEFORE' -> '$FRONTIER_AFTER')" >&2
  rc=1
else
  echo "ok: current_focus preserved"
fi

if [ "$rc" -eq 0 ]; then
  echo "PASS: ordered refresh drill ($PROVIDER)"
else
  echo "FAIL: ordered refresh drill ($PROVIDER)" >&2
fi
exit "$rc"
