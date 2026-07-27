#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
# shellcheck source=/dev/null
source "$ROOT/evals/_lib/lib.sh"

RUN_ID="$(date +%s)-$$"
OUT="$ROOT/target-check/evals/plan-lifecycle/$RUN_ID"
SPEC="$OUT/spec.md"
mkdir -p "$OUT"

dev_alive || { echo "dev daemon not on port 7686; start it with ./build.sh dev" >&2; exit 1; }
# Snapshot before creating anything; sweep on ANY exit (this eval otherwise
# leaves an archived+disowned PLAN-LIFECYCLE-* plan behind on every run).
PLAN_BASELINE="$OUT/plan-baseline.txt"
plan_snapshot "$PLAN_BASELINE" || { echo "could not snapshot plan baseline" >&2; exit 1; }
cleanup() { plan_sweep_new "${PLAN_BASELINE:-}" >/dev/null 2>&1 || true; }
trap cleanup EXIT

fail() {
  echo "  ✗ $*" >&2
  exit 1
}

pass() {
  echo "  ✓ $*"
}

need() {
  command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

json_get() {
  dc --json plan get "$1"
}

assert_jq() {
  local json="$1" label="$2"
  shift 2
  jq -e "$@" <<<"$json" >/dev/null || fail "$label"
  pass "$label"
}

assert_error_contains() {
  local label="$1" needle="$2"
  shift 2
  local out rc
  set +e
  out="$("$@" 2>&1)"
  rc=$?
  set -e
  [ "$rc" -ne 0 ] || fail "$label: command unexpectedly succeeded"
  grep -Fq "$needle" <<<"$out" || fail "$label: missing '$needle' in: $out"
  pass "$label"
}

need jq
dev_alive || fail "dev daemon not running on port 7686; start it with ./build.sh dev --wait"

cat > "$SPEC" <<'EOF'
# Canvas lifecycle fixture

## Scope

Build the smallest useful plan lifecycle fixture.

## Verification

Use jq assertions against the daemon state.
EOF

echo "== plan lifecycle eval =="

CREATE_JSON="$(dc --json plan create --title "PLAN-LIFECYCLE-$RUN_ID" --spec-file "$SPEC" --kinds status_update,review)"
PLAN_ID="$(jq -r '.plan_id' <<<"$CREATE_JSON")"
[ -n "$PLAN_ID" ] && [ "$PLAN_ID" != "null" ] || fail "plan create did not return plan_id"
pass "created plan $PLAN_ID"

PLAN_JSON="$(json_get "$PLAN_ID")"
assert_jq "$PLAN_JSON" "spec file produced plan sections" '.sections | length >= 1'
SPEC_SECTION="$(jq -r '.sections[0].id' <<<"$PLAN_JSON")"

IMPL_SECTION="$(dc plan section add "$PLAN_ID" --title Implementation --body="Implementation lane")"
[ -n "$IMPL_SECTION" ] || fail "section add did not return an id"
pass "added implementation section $IMPL_SECTION"

TASK_ID="$(dc plan task add "$PLAN_ID" \
  --name "Implement fixture task" \
  --type implement \
  --sections "$SPEC_SECTION,$IMPL_SECTION" \
  --body="Touch both sections" \
  --intent="Keep the eval scoped")"
[ -n "$TASK_ID" ] || fail "task add did not return an id"
pass "added task $TASK_ID"

PLAN_JSON="$(json_get "$PLAN_ID")"
assert_jq "$PLAN_JSON" \
  "task is linked to both sections" \
  --arg task "$TASK_ID" --arg s1 "$SPEC_SECTION" --arg s2 "$IMPL_SECTION" \
  '.tasks[] | select(.id == $task) | (.section_ids | index($s1)) and (.section_ids | index($s2))'

dc plan task assign "$PLAN_ID" "$TASK_ID" --tab "plan-lifecycle-worker" >/dev/null
PLAN_JSON="$(json_get "$PLAN_ID")"
assert_jq "$PLAN_JSON" \
  "task assignment persisted" \
  --arg task "$TASK_ID" \
  '.tasks[] | select(.id == $task) | .assigned_tab_id == "plan-lifecycle-worker"'

dc agent report "Lifecycle milestone appended" --tab "plan-lifecycle-worker" --plan "$PLAN_ID" --task "$TASK_ID" --status done >/dev/null
PLAN_JSON="$(json_get "$PLAN_ID")"
assert_jq "$PLAN_JSON" \
  "agent report appended a milestone" \
  --arg task "$TASK_ID" \
  '.tasks[] | select(.id == $task) | (.milestones | length >= 1)'

dc plan task close "$PLAN_ID" "$TASK_ID" --result="Fixture task complete" >/dev/null
PLAN_JSON="$(json_get "$PLAN_ID")"
assert_jq "$PLAN_JSON" \
  "task close marked done" \
  --arg task "$TASK_ID" \
  '.tasks[] | select(.id == $task) | .status == "done"'

LOOP_ID="$(dc plan loop create "$PLAN_ID" --target "$TASK_ID" --budget 1 --stop-when="all checks pass")"
[ -n "$LOOP_ID" ] || fail "loop create did not return an id"
pass "created loop $LOOP_ID"

assert_error_contains "bad milestone kind teaching" \
  "codeterm plan kinds set $PLAN_ID" \
  dc plan milestone add "$PLAN_ID" "$TASK_ID" --kind qa --text="bad kind"

assert_error_contains "bad task status teaching" \
  "pending|in_progress|blocked|done|cancelled" \
  dc plan task update "$PLAN_ID" "$TASK_ID" --status almost_done

assert_error_contains "unknown plan teaching" \
  "codeterm plan list --active" \
  dc plan get "pl_missing_$RUN_ID"

assert_error_contains "dash-leading text teaching" \
  "--field=value or --field-file" \
  dc plan task add "$PLAN_ID" --name "dash body" --body - starts-with-dash

assert_error_contains "loop budget teaching" \
  "codeterm plan loop set $LOOP_ID --budget N" \
  dc plan loop set "$LOOP_ID" --budget 0

dc plan archive "$PLAN_ID" >/dev/null
dc plan disown "$PLAN_ID" >/dev/null
UNOWNED="$(dc --json plan list --unowned)"
jq -e --arg plan "$PLAN_ID" '.plans[] | select(.plan_id == $plan)' <<<"$UNOWNED" >/dev/null \
  || fail "archived disowned plan is not listed by --unowned"
pass "archived disowned plan appears in --unowned"

# Delete the fixture (the --unowned assertion above needed it alive), then
# assert the plan set returned to baseline — no PLAN-LIFECYCLE-* junk left.
plan_delete_one "$PLAN_ID" >/dev/null 2>&1 || true
plan_sweep_new "$PLAN_BASELINE" >/dev/null 2>&1 || true
plan_assert_clean "$PLAN_BASELINE" plan-lifecycle || fail "plan lifecycle leaked plans (before != after)"

echo "== plan lifecycle eval passed =="
