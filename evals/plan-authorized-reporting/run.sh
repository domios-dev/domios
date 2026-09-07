#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
JSON="$ROOT/evals/plan-authorized-reporting/json.mjs"

fail() {
  echo "FAIL $1" >&2
  exit 1
}

[ "${1:-}" = "--check" ] || {
  echo "usage: bash evals/plan-authorized-reporting/run.sh --check" >&2
  exit 2
}
command -v node >/dev/null 2>&1 || fail "prerequisite: node is required"
command -v python3 >/dev/null 2>&1 || fail "prerequisite: python3 is required"
[ -f "$ROOT/evals/_lib/lib.sh" ] || fail "prerequisite: eval library is missing"
if ! source "$ROOT/evals/_lib/lib.sh"; then
  fail "prerequisite: eval library could not be loaded"
fi

TMP_DIR="$(mktemp -d)" || fail "prerequisite: temporary directory creation failed"
RUN_ID="$(basename "$TMP_DIR")-${BASHPID}"
PRIMARY_CWD="$TMP_DIR/primary-$RUN_ID"
FOREIGN_CWD="$TMP_DIR/foreign-$RUN_ID"
TMP_MARKER="$TMP_DIR/.plan-authorized-reporting-owned"
mkdir -p "$PRIMARY_CWD" "$FOREIGN_CWD" || fail "fixture_cwd: creation failed"
touch "$TMP_MARKER" || fail "fixture_cwd: ownership marker failed"

OWNED_WORKSPACES=()
CREATED_TABS=()
CREATED_SESSIONS=()
PLAN_ID=""
PLAN_OWNED=0
TEAM_ID=""
TEAM_OWNED=0
OWNER_TAB=""
OWNER_SESSION="owner-session-$RUN_ID"
OWNER_NONCE=""
EVENT_PID=""
EVENT_FD=""

cleanup() {
  local status=$? cleanup_failed=0 workspace_id
  trap - EXIT
  if [ -n "$EVENT_PID" ]; then
    kill "$EVENT_PID" >/dev/null 2>&1 || true
    wait "$EVENT_PID" >/dev/null 2>&1 || true
  fi
  if [ "$PLAN_OWNED" -eq 1 ]; then
    if ! CODETERM_SESSION_ID="$OWNER_SESSION" CODETERM_SESSION_BINDING_NONCE="$OWNER_NONCE" \
      dc --as-tab "$OWNER_TAB" plan delete "$PLAN_ID" --yes >/dev/null 2>&1; then
      echo "FAIL cleanup: owned plan $PLAN_ID was not deleted" >&2
      cleanup_failed=1
    fi
  fi
  if [ "$TEAM_OWNED" -eq 1 ]; then
    if ! CODETERM_SESSION_ID="$OWNER_SESSION" CODETERM_SESSION_BINDING_NONCE="$OWNER_NONCE" \
      dc --as-tab "$OWNER_TAB" team close "$TEAM_ID" >/dev/null 2>&1; then
      echo "FAIL cleanup: owned Team $TEAM_ID was not closed" >&2
      cleanup_failed=1
    fi
  fi
  for workspace_id in "${OWNED_WORKSPACES[@]}"; do
    if ! dc workspace close "$workspace_id" --force >/dev/null 2>&1; then
      echo "FAIL cleanup: owned workspace $workspace_id was not closed" >&2
      cleanup_failed=1
    fi
  done
  if [ -f "$TMP_MARKER" ]; then
    rm -rf -- "$TMP_DIR" || cleanup_failed=1
  else
    echo "FAIL cleanup: temporary ownership guard is missing" >&2
    cleanup_failed=1
  fi
  if [ "$status" -eq 0 ] && [ "$cleanup_failed" -ne 0 ]; then
    status=1
  fi
  exit "$status"
}
trap cleanup EXIT

capture() {
  local result_var="$1" output
  shift
  if ! output="$("$@")"; then
    return 1
  fi
  printf -v "$result_var" '%s' "$output"
}

create_workspace() {
  local result_var="$1" name="$2" cwd="$3" response workspace_id
  if ! capture response dc workspace create "$name" --cwd "$cwd" --json; then
    fail "fixture_workspace: create failed for $name"
  fi
  if ! capture workspace_id node "$JSON" workspace-owned "$cwd" <<<"$response"; then
    fail "fixture_workspace: $name was reused or returned no owned id"
  fi
  OWNED_WORKSPACES+=("$workspace_id")
  printf -v "$result_var" '%s' "$workspace_id"
}

new_tab() {
  local result_var="$1" title="$2" workspace_id="$3" cwd="$4" response tab_id
  if ! capture response dc tab new --workspace "$workspace_id" --cwd "$cwd" \
    --title "$title" --no-team --eager --json; then
    fail "fixture_tab: create failed for $title"
  fi
  if ! capture tab_id node "$JSON" tab-id <<<"$response" || [ -z "$tab_id" ]; then
    fail "fixture_tab: missing id for $title"
  fi
  CREATED_TABS+=("$tab_id")
  printf -v "$result_var" '%s' "$tab_id"
}

tab_nonce() {
  local result_var="$1" tab_id="$2" response nonce
  if ! capture response dc tab inspect --tab "$tab_id" --json; then
    fail "fixture_binding: inspect failed for $tab_id"
  fi
  if ! capture nonce node "$JSON" tab-nonce <<<"$response" || [ -z "$nonce" ]; then
    fail "fixture_binding: missing nonce for $tab_id"
  fi
  printf -v "$result_var" '%s' "$nonce"
}

bind_tab() {
  local result_var="$1" tab_id="$2" session_id="$3" nonce body response
  tab_nonce nonce "$tab_id"
  if ! capture body node "$JSON" session-body "$session_id" "$nonce" </dev/null; then
    fail "fixture_binding: request encoding failed for $tab_id"
  fi
  if ! capture response dev_post "/api/tabs/$tab_id/hook/session_start" "$body"; then
    fail "fixture_binding: session_start failed for $tab_id"
  fi
  if ! node "$JSON" bind-ok "$session_id" <<<"$response"; then
    fail "fixture_binding: session_start was not acknowledged for $tab_id"
  fi
  CREATED_SESSIONS+=("$session_id")
  printf -v "$result_var" '%s' "$nonce"
}

plan_json() {
  local result_var="$1" response
  if ! capture response dc plan get "$PLAN_ID" --json; then
    fail "assertion: plan $PLAN_ID could not be read"
  fi
  printf -v "$result_var" '%s' "$response"
}

token_absent_everywhere() {
  local token="$1" current_plan reports
  plan_json current_plan
  node "$JSON" token-absent "$token" <<<"$current_plan" || return 1
  capture reports dev_get "/api/tabs/$OWNER_TAB/reports" || return 1
  node "$JSON" report-token-absent "$token" <<<"$reports"
}

start_report_subscription() {
  local marker="$1" output="$TMP_DIR/event.json" ready
  rm -f "$output"
  coproc REPORT_EVENT {
    python3 "$ROOT/evals/plan-authorized-reporting/event_wait.py" \
      "$(dev_api_url)" "$(dev_token)" agent-report "$marker" "$output" 20
  }
  EVENT_PID="$REPORT_EVENT_PID"
  EVENT_FD="${REPORT_EVENT[0]}"
  if ! read -r -t 10 ready <&"$EVENT_FD" || [ "$ready" != "READY" ]; then
    fail "subscription: agent-report readiness was not observed"
  fi
}

finish_report_subscription() {
  if ! wait "$EVENT_PID"; then
    fail "subscription: matching agent-report event was not observed"
  fi
  EVENT_PID=""
  EVENT_FD=""
  node "$JSON" event-type <"$TMP_DIR/event.json" || \
    fail "assertion: subscribed event had the wrong type"
}

assert_refused() {
  local label="$1" tab="$2" session="$3" nonce="$4" token="$5"
  local current_plan before after output rc
  plan_json current_plan
  capture before node "$JSON" plan-snapshot <<<"$current_plan" || \
    fail "$label: before snapshot failed"
  set +e
  output="$({
    unset CODETERM_PLAN_ID CODETERM_TASK_ID
    CODETERM_SESSION_ID="$session" CODETERM_SESSION_BINDING_NONCE="$nonce" \
      dc --as-tab "$tab" agent report "$token" --plan "$PLAN_ID" --json
  } 2>&1)"
  rc=$?
  set -e
  [ "$rc" -ne 0 ] || fail "$label: report accepted"
  [ -n "$output" ] || fail "$label: refusal had no actionable output"
  plan_json current_plan
  capture after node "$JSON" plan-snapshot <<<"$current_plan" || \
    fail "$label: after snapshot failed"
  [ "$before" = "$after" ] || fail "$label: plan evidence or task state changed"
  token_absent_everywhere "$token" || fail "$label: token persisted or delivered"
  echo "PASS $label"
}

dev_get /api/health >/dev/null || fail "prerequisite: dev API is unavailable"

create_workspace PRIMARY_WORKSPACE "reporting-primary-$RUN_ID" "$PRIMARY_CWD"
create_workspace FOREIGN_WORKSPACE "reporting-foreign-$RUN_ID" "$FOREIGN_CWD"
new_tab OWNER_TAB "reporting-owner-$RUN_ID" "$PRIMARY_WORKSPACE" "$PRIMARY_CWD"
bind_tab OWNER_NONCE "$OWNER_TAB" "$OWNER_SESSION"
if ! CODETERM_SESSION_ID="$OWNER_SESSION" CODETERM_SESSION_BINDING_NONCE="$OWNER_NONCE" \
  dc --as-tab "$OWNER_TAB" agent set-orchestrator --json >/dev/null; then
  fail "fixture_owner: orchestrator registration failed"
fi

if ! PLAN_RESPONSE="$(
  CODETERM_SESSION_ID="$OWNER_SESSION" CODETERM_SESSION_BINDING_NONCE="$OWNER_NONCE" \
    dc --as-tab "$OWNER_TAB" plan create \
      --title "plan-authorized-reporting-$RUN_ID" --project "$PRIMARY_CWD" --json
)"; then
  fail "fixture_plan: create failed"
fi
capture PLAN_ID node "$JSON" plan-id <<<"$PLAN_RESPONSE" || fail "fixture_plan: missing id"
[ -n "$PLAN_ID" ] || fail "fixture_plan: missing id"
PLAN_OWNED=1
capture TEAM_ID node "$JSON" team-id <<<"$PLAN_RESPONSE" || fail "fixture_team: missing id"
[ -n "$TEAM_ID" ] || fail "fixture_team: missing id"
TEAM_OWNED=1
node "$JSON" team-minted <<<"$PLAN_RESPONSE" || fail "fixture_team: Team was not uniquely minted"

QA_SESSION="qa-session-$RUN_ID"
FOREIGN_SESSION="foreign-session-$RUN_ID"
BOUND_SESSION="bound-session-$RUN_ID"
UNBOUND_SESSION="unbound-session-$RUN_ID"
new_tab QA_TAB "authorized-canonical-qa-$RUN_ID" "$PRIMARY_WORKSPACE" "$PRIMARY_CWD"
bind_tab QA_NONCE "$QA_TAB" "$QA_SESSION"
new_tab FOREIGN_TAB "foreign-reviewer-$RUN_ID" "$FOREIGN_WORKSPACE" "$FOREIGN_CWD"
bind_tab FOREIGN_NONCE "$FOREIGN_TAB" "$FOREIGN_SESSION"
new_tab UNBOUND_TAB "unbound-reviewer-$RUN_ID" "$PRIMARY_WORKSPACE" "$PRIMARY_CWD"
tab_nonce UNBOUND_NONCE "$UNBOUND_TAB"
new_tab BOUND_TAB "task-bound-reviewer-$RUN_ID" "$PRIMARY_WORKSPACE" "$PRIMARY_CWD"
bind_tab BOUND_NONCE "$BOUND_TAB" "$BOUND_SESSION"

if ! CODETERM_SESSION_ID="$OWNER_SESSION" CODETERM_SESSION_BINDING_NONCE="$OWNER_NONCE" \
  dc --as-tab "$OWNER_TAB" plan review authorize-reporter "$PLAN_ID" "$QA_TAB" \
    --class canonical_qa --json >/dev/null; then
  fail "fixture_authorization: canonical QA grant failed"
fi
if ! CODETERM_SESSION_ID="$OWNER_SESSION" CODETERM_SESSION_BINDING_NONCE="$OWNER_NONCE" \
  dc --as-tab "$OWNER_TAB" plan review authorize-reporter "$PLAN_ID" "$BOUND_TAB" \
    --class review --json >/dev/null; then
  fail "fixture_authorization: task-bound grant failed"
fi

if ! TASK_RESPONSE="$(
  CODETERM_SESSION_ID="$OWNER_SESSION" CODETERM_SESSION_BINDING_NONCE="$OWNER_NONCE" \
    dc --as-tab "$OWNER_TAB" plan task add "$PLAN_ID" \
      --name "task-bound misuse sentinel $RUN_ID" --executor-role "oracle-worker" \
      --role-guidance "opaque-fixture-$RUN_ID" --criterion "opaque-criterion-$RUN_ID" \
      --test-plan "opaque-test-$RUN_ID" --assign-tab "$BOUND_TAB" --json
)"; then
  fail "fixture_task: create failed"
fi
capture TASK_ID node "$JSON" task-id <<<"$TASK_RESPONSE" || fail "fixture_task: missing id"
[ -n "$TASK_ID" ] || fail "fixture_task: missing id"

echo "FIXTURE run=$RUN_ID"
echo "FIXTURE workspace.primary=$PRIMARY_WORKSPACE cwd=$PRIMARY_CWD owned=unique_cwd"
echo "FIXTURE workspace.foreign=$FOREIGN_WORKSPACE cwd=$FOREIGN_CWD owned=unique_cwd"
echo "FIXTURE plan=$PLAN_ID team=$TEAM_ID owner_tab=$OWNER_TAB owner_session=$OWNER_SESSION"
echo "FIXTURE tab.qa=$QA_TAB session=$QA_SESSION"
echo "FIXTURE tab.foreign=$FOREIGN_TAB session=$FOREIGN_SESSION"
echo "FIXTURE tab.unbound=$UNBOUND_TAB session=$UNBOUND_SESSION"
echo "FIXTURE tab.task_bound=$BOUND_TAB session=$BOUND_SESSION task=$TASK_ID"

plan_json CURRENT_PLAN
capture TASK_BEFORE node "$JSON" task-snapshot "$TASK_ID" <<<"$CURRENT_PLAN" || \
  fail "assertion: initial task snapshot failed"

ACCEPTED_TOKEN="qa-evidence-$RUN_ID"
start_report_subscription "$ACCEPTED_TOKEN"
if ! ACCEPTED="$(
  unset CODETERM_PLAN_ID CODETERM_TASK_ID
  CODETERM_SESSION_ID="$QA_SESSION" CODETERM_SESSION_BINDING_NONCE="$QA_NONCE" \
    dc --as-tab "$QA_TAB" agent report "$ACCEPTED_TOKEN" --plan "$PLAN_ID" --json
)"; then
  fail "assertion: authorized QA report was refused"
fi
finish_report_subscription
node "$JSON" accepted-response "$PLAN_ID" <<<"$ACCEPTED" || \
  fail "assertion: authorized response contract failed"
plan_json CURRENT_PLAN
node "$JSON" plan-evidence "$ACCEPTED_TOKEN" <<<"$CURRENT_PLAN" || \
  fail "assertion: authorized token was not persisted"
capture OWNER_REPORTS dev_get "/api/tabs/$OWNER_TAB/reports" || \
  fail "assertion: owner report channel could not be read"
node "$JSON" owner-report "$ACCEPTED_TOKEN" <<<"$OWNER_REPORTS" || \
  fail "assertion: authorized token did not reach the owner"
capture TASK_AFTER node "$JSON" task-snapshot "$TASK_ID" <<<"$CURRENT_PLAN" || \
  fail "assertion: final task snapshot failed"
[ "$TASK_BEFORE" = "$TASK_AFTER" ] || fail "authorized_qa_task_non_mutation"
echo "PASS authorized_qa_persistence_and_delivery"

assert_refused foreign_tab "$FOREIGN_TAB" "$FOREIGN_SESSION" "$FOREIGN_NONCE" \
  "foreign-refused-$RUN_ID"
assert_refused unbound_tab "$UNBOUND_TAB" "$UNBOUND_SESSION" "$UNBOUND_NONCE" \
  "unbound-refused-$RUN_ID"
assert_refused stale_rebound_identity "$QA_TAB" "stale-session-$RUN_ID" \
  "stale-nonce-$RUN_ID" "stale-refused-$RUN_ID"
assert_refused task_bound_misuse "$BOUND_TAB" "$BOUND_SESSION" "$BOUND_NONCE" \
  "task-bound-refused-$RUN_ID"

echo "PASS plan_authorized_reporting"
