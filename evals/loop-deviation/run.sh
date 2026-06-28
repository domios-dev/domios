#!/usr/bin/env bash
set -euo pipefail

# Behaviour bench: a stagnating loop trips a goal-anchored deviation directive;
# a progressing loop never deviates. Deterministic — the engine is deterministic.
# Drives the REAL dev daemon on 7686 via the dc/dev_* helpers (never prod).

ROOT="$(git rev-parse --show-toplevel)"
# shellcheck source=/dev/null
source "$ROOT/evals/_lib/lib.sh"

RUN_ID="$(date +%s)-$$"
GOAL="ship a self-correcting loop engine"
TARGET="make the deviation engine converge"

fail() { echo "  ✗ $*" >&2; exit 1; }
pass() { echo "  ✓ $*"; }
need() { command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"; }

jqf() { jq -e "$@" >/dev/null || fail "assertion failed: $*"; }

need jq
dev_alive || fail "dev daemon not on port 7686; start it with ./build.sh dev --wait"

echo "== loop-deviation eval =="

PLAN_ID="$(jq -r '.plan_id' <<<"$(dc --json plan create --title "LOOP-DEVIATION-$RUN_ID")")"
[ -n "$PLAN_ID" ] && [ "$PLAN_ID" != "null" ] || fail "plan create did not return plan_id"
pass "created scratch plan $PLAN_ID"

cleanup() { dc plan delete "$PLAN_ID" >/dev/null 2>&1 || true; }
trap cleanup EXIT

dc plan goal set "$PLAN_ID" --vector "$GOAL" >/dev/null
pass "set goal vector"

# --- Scenario A: a stagnating loop deviates after K consecutive no-progress iters
LID="$(dc plan loop create "$PLAN_ID" --target "$TARGET" --budget 8 --stop-when "stop_when met")"
[ -n "$LID" ] || fail "loop create did not return an id"
pass "created loop $LID (budget 8)"

A1="$(dc --json plan loop advance "$LID" --progress false)"
jqf '.stagnation == 1' <<<"$A1"
jqf '.deviation == null' <<<"$A1"
pass "iter 1 no-progress: stagnation=1, no deviation yet (below K)"

A2="$(dc --json plan loop advance "$LID" --progress false)"
jqf '.stagnation == 2' <<<"$A2"
jqf '.deviation != null' <<<"$A2"
jqf --arg t "$TARGET" '.deviation | contains($t)' <<<"$A2"
jqf --arg g "$GOAL" '.deviation | contains($g)' <<<"$A2"
pass "iter 2 no-progress: stagnation=2, deviation fired and references target + goal"
echo "    deviation: $(jq -r '.deviation' <<<"$A2")"

# --- Scenario B: progress after stagnation resets and clears the deviation
A3="$(dc --json plan loop advance "$LID" --progress true)"
jqf '.stagnation == 0' <<<"$A3"
jqf '.deviation == null' <<<"$A3"
pass "progress resets stagnation to 0 and clears deviation"

# --- Scenario C: a never-stagnating loop never deviates
LID2="$(dc plan loop create "$PLAN_ID" --target "$TARGET" --budget 8 --stop-when "stop_when met")"
for i in 1 2 3; do
  R="$(dc --json plan loop advance "$LID2" --progress true)"
  jqf '.stagnation == 0' <<<"$R"
  jqf '.deviation == null' <<<"$R"
done
pass "progressing loop never deviates across 3 iters"

# --- Guard: an exhausted loop refuses to advance (budget bound holds)
LID3="$(dc plan loop create "$PLAN_ID" --target "$TARGET" --budget 1 --stop-when "stop_when met")"
dc plan loop advance "$LID3" --progress false >/dev/null
if dc --json plan loop advance "$LID3" --progress false >/dev/null 2>&1; then
  fail "exhausted loop advanced past budget"
fi
pass "budget guard: exhausted loop refuses further advance"

echo "== loop-deviation eval passed =="
