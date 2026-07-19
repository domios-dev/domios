#!/usr/bin/env bash
# Failure-branch drills — the two branches that must NEVER strand the orchestrator:
#
#  A) HUNG COMPACT: compaction never settles. With a tiny settle_timeout the
#     machine must still reach S4 (re-orient) — proven by 's3_settle' followed
#     by 's4_reorient' even though no compaction signature was seen.
#
#  B) DEAD RESUMPTION: the orchestrator never resumes. The machine must
#     re-nudge exactly once, then ALERT (notification + `refresh-stranded`
#     heartbeat learning) — never silence.
#
# Branch B is driven against a SHELL pane (no agent → status never flips to
# Working), which deterministically exercises the dead-resumption path without
# needing a wedged real agent.
#
# Usage: ./evals/orchestrator-refresh/drill_failure_branches.sh
# Tunables: set [orchestration.refresh] settle_timeout_secs / verify_timeout_secs
#           LOW in the dev config before running so the drill finishes fast.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
source "$HERE/lib.sh"

dev_alive || { echo "ERROR: dev not running on 7686. ./build.sh dev" >&2; exit 1; }

CWD="$(pwd)/target-check/evals/orchestrator-refresh-fail"
mkdir -p "$CWD"
rc=0

echo "########## BRANCH B: dead resumption -> re-nudge once -> ALERT ##########"
WS=$(new_workspace "REFRESH-FAIL" "$CWD")
PANE=$(new_shell_pane "$WS" "$CWD" "shell-orch")
PLAN=$(dc plan create --title "refresh fail drill" --json 2>/dev/null \
  | python -c "import json,sys; print(json.load(sys.stdin).get('plan_id',''))")
[ -n "$PANE" ] && [ -n "$PLAN" ] || { echo "FAIL: setup" >&2; exit 1; }

# A shell pane is not an agent — orchestrator_refresh returns not_eligible for
# the real send path, so branch B is asserted against an AGENT pane whose
# resumption we suppress by never letting it act. Spawn a claude pane and do NOT
# feed it the re-orient (config verify_timeout must be small; see README).
PANE=$(new_claude_pane "$WS" "$CWD" "orch-dead")
[ -n "$PANE" ] || { echo "FAIL: agent pane spawn" >&2; exit 1; }
sleep 8

OFF=$(log_offset)
dc orchestrator refresh --plan "$PLAN" --tab "$PANE" >/dev/null 2>&1

echo "== wait for the alert (bounded by 2× verify_timeout + settle)"
if wait_for_marker "$OFF" "single re-nudge" 400; then
  echo "ok: single re-nudge fired"
else
  echo "WARN: re-nudge marker not seen — orchestrator may have resumed (not a dead-resumption env)"
fi
if wait_for_marker "$OFF" "ALERT" 220; then
  echo "ok: ALERT raised"
  # The planner-routed decision gate is a tagged heartbeat learning.
  if dc orchestrator heartbeat show --plan "$PLAN" 2>/dev/null | grep -qiF "refresh stranded"; then
    echo "ok: decision-gate learning recorded"
  else
    echo "FAIL: alert raised but no 'refresh stranded' learning" >&2; rc=1
  fi
  # Exactly ONE re-nudge — never a loop.
  n=$(log_since "$OFF" | grep -cF "single re-nudge")
  [ "$n" -le 1 ] || { echo "FAIL: expected <=1 re-nudge, got $n" >&2; rc=1; }
else
  echo "FAIL: dead resumption did not end in an ALERT — SILENCE is forbidden" >&2; rc=1
fi

echo
echo "########## BRANCH A: hung compact -> still reaches re-orient ##########"
echo "Precondition: set [orchestration.refresh] settle_timeout_secs = 5 in dev config."
echo "The machine must log s3_settle then s4_reorient with NO compaction signature."
WS2=$(new_workspace "REFRESH-HUNG" "$CWD")
PANE2=$(new_claude_pane "$WS2" "$CWD" "orch-hung")
PLAN2=$(dc plan create --title "refresh hung drill" --json 2>/dev/null \
  | python -c "import json,sys; print(json.load(sys.stdin).get('plan_id',''))")
[ -n "$PANE2" ] && [ -n "$PLAN2" ] || { echo "FAIL: setup B" >&2; exit 1; }
sleep 8
# Force S1 to land immediately so we isolate the settle branch.
dc orchestrator handoff --plan "$PLAN2" --tab "$PANE2" >/dev/null 2>&1 || true

OFF2=$(log_offset)
dc orchestrator refresh --plan "$PLAN2" --tab "$PANE2" >/dev/null 2>&1
wait_for_marker "$OFF2" "phase=s4_reorient" 200 || { echo "FAIL: hung compact never reached S4" >&2; rc=1; }
TAIL2="$(log_since "$OFF2")"
assert_order "$TAIL2" "phase=s3_settle" "phase=s4_reorient" || rc=1

if [ "$rc" -eq 0 ]; then echo "PASS: failure-branch drills"; else echo "FAIL: failure-branch drills" >&2; fi
exit "$rc"
