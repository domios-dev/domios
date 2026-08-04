#!/usr/bin/env bash
# Ordered-transcript drill: the PROOF that `codeterm orchestrator refresh` runs
# S1..S5 in strict order. Asserts:
#   handoff-stride ts  <  compact cmd  <  compaction signature settle
#     <  re-orient injection  <  first post-refresh plan-state read (resumed)
# and that plan state remains readable after resumption.
#
# Provider is selectable: PROVIDER=claude (default) | codex. Codex usage may be
# exhausted — the orchestrator schedules the codex run when headroom returns.
#
# Usage: PROVIDER=claude ./evals/orchestrator-refresh/drill.sh
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
source "$HERE/lib.sh"
# shellcheck source=/dev/null
source "$HERE/../_lib/role_preset.sh"

dev_alive || { echo "ERROR: dev not running on 7686. ./build.sh dev" >&2; exit 1; }

# A refreshed Orchestrator must re-orient from the same resolver it launched
# with; a drifted kind here would silently reseed a different charter.
assert_manager_kinds_resolve || { echo "ERROR: manager preset resolution drifted" >&2; exit 1; }
assert_planner_team_authority || { echo "ERROR: role kind capability drifted" >&2; exit 1; }

PROVIDER="${PROVIDER:-claude}"
CWD="$(pwd)/target-check/evals/orchestrator-refresh"
mkdir -p "$CWD"

# Realistic timeouts for a LIVE agent: /compact alone takes ~40s; a fresh agent
# needs seconds to read+run each injected command. Tight caps strand the cycle.
dc config set orchestration.refresh.handoff_timeout_secs 60 >/dev/null 2>&1
dc config set orchestration.refresh.settle_timeout_secs 40 >/dev/null 2>&1
dc config set orchestration.refresh.verify_timeout_secs 90 >/dev/null 2>&1

echo "== [1/6] spawn a $PROVIDER orchestrator tab + bind a plan"
WS=$(new_workspace "REFRESH-DRILL" "$CWD")
[ -n "$WS" ] || { echo "FAIL: workspace create" >&2; exit 1; }

case "$PROVIDER" in
  claude) TAB=$(new_claude_tab "$WS" "$CWD" "orch") ;;
  codex)  TAB=$(new_codex_tab  "$WS" "$CWD" "orch") ;;
  *) echo "FAIL: unknown PROVIDER=$PROVIDER" >&2; exit 1 ;;
esac
[ -n "$TAB" ] || { echo "FAIL: tab spawn ($PROVIDER)" >&2; exit 1; }
echo "   tab=$TAB"

PLAN=$(dc plan create --title "refresh drill" --json 2>/dev/null \
  | python -c "import json,sys; print(json.load(sys.stdin).get('plan_id',''))")
[ -n "$PLAN" ] || { echo "FAIL: plan create" >&2; exit 1; }
echo "   plan=$PLAN"

echo "== [2/6] wait for the tab to reach READY, then prime its orchestrator contract"
# Real readiness signal (learned by manual walk): the daemon logs 'agent ready in
# tab X' ~14s after spawn. A fixed sleep races the boot and the S1 injection lands
# in a not-yet-ready agent that never submits it.
for _ in $(seq 1 30); do
  grep -qaE "agent ready in tab $TAB" "$DEV_LOG" 2>/dev/null && break
  sleep 2
done
sleep 3
# A BARE agent strands the cycle — prime it to run the exact command each injection names.
dc send --tab "$TAB" "You are a test orchestrator. Stay idle at your prompt. When you receive an injected message naming an exact shell command (e.g. codeterm orchestrator handoff --plan ..., or codeterm plan state ...), run THAT exact command verbatim, using the plan id written in the message. Do nothing else." >/dev/null 2>&1
sleep 2

OFF=$(log_offset)

echo "== [3/6] trigger refresh"
dc orchestrator refresh --plan "$PLAN" --tab "$TAB" || {
  echo "FAIL: refresh trigger returned non-zero" >&2; exit 1; }

echo "== [4/6] wait for the terminal marker (resumed or alert)"
if ! wait_for_marker "$OFF" "[refresh] tab=" 30; then
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

echo "== [6/6] assert continuity: plan state remains readable"
dc plan state "$PLAN" >/dev/null || rc=1

if [ "$rc" -eq 0 ]; then
  echo "PASS: ordered refresh drill ($PROVIDER)"
else
  echo "FAIL: ordered refresh drill ($PROVIDER)" >&2
fi
exit "$rc"
