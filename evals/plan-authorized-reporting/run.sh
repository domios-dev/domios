#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$ROOT/evals/_lib/lib.sh"

[ "${1:-}" = "--check" ] || {
  echo "usage: bash evals/plan-authorized-reporting/run.sh --check" >&2
  exit 2
}

WORKSPACE_ID=""
PLAN_ID=""
OWNER_TAB=""
OWNER_SESSION="owner-session-91f0c4"
OWNER_NONCE=""
EVENT_PID=""
TMP_DIR="$(mktemp -d)"

cleanup() {
  [ -z "$EVENT_PID" ] || kill "$EVENT_PID" >/dev/null 2>&1 || true
  if [ -n "$PLAN_ID" ] && [ -n "$OWNER_TAB" ] && [ -n "$OWNER_NONCE" ]; then
    CODETERM_SESSION_ID="$OWNER_SESSION" CODETERM_SESSION_BINDING_NONCE="$OWNER_NONCE" \
      dc --as-tab "$OWNER_TAB" plan delete "$PLAN_ID" --yes >/dev/null 2>&1 || true
  fi
  [ -z "$WORKSPACE_ID" ] || dc workspace close "$WORKSPACE_ID" >/dev/null 2>&1 || true
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

capture_json() {
  local output
  output="$("$@")"
  printf '%s\n' "$output"
}

tab_id_from_create() {
  jq -r '.tab_id // .id // empty'
}

plan_id_from_create() {
  jq -r '.plan.id // .id // empty'
}

tab_nonce() {
  jq -r 'if type == "array" then .[0] else (.tabs[0]? // .) end | .session_binding_nonce // empty'
}

bind_tab() {
  local tab_id="$1" session_id="$2" nonce body response
  nonce="$(capture_json dc tab inspect --tab "$tab_id" --json | tab_nonce)"
  [ -n "$nonce" ] || { echo "FAIL fixture_binding: missing nonce for $tab_id"; exit 1; }
  body="$(jq -nc --arg session "$session_id" --arg nonce "$nonce" \
    '{session_id:$session,binding_nonce:$nonce,source:"plan-authorized-reporting"}')"
  response="$(dev_post "/api/tabs/$tab_id/hook/session_start" "$body")"
  jq -e --arg session "$session_id" '.ok == true and .session_id == $session' \
    >/dev/null <<<"$response"
  printf '%s\n' "$nonce"
}

new_tab() {
  local title="$1" response tab
  response="$(capture_json dc tab new --workspace "$WORKSPACE_ID" --cwd "$ROOT" --title "$title" --eager --json)"
  tab="$(tab_id_from_create <<<"$response")"
  [ -n "$tab" ] || { echo "FAIL fixture_tab: $title"; exit 1; }
  printf '%s\n' "$tab"
}

plan_json() {
  capture_json dc plan get "$PLAN_ID" --json
}

token_absent_everywhere() {
  local token="$1"
  ! plan_json | jq -e --arg token "$token" '.. | strings | select(. == $token)' >/dev/null
  ! dev_get "/api/tabs/$OWNER_TAB/reports" | jq -e --arg token "$token" \
    '.[] | select(.message == $token)' >/dev/null
}

start_report_subscription() {
  local marker="$1" ready="$TMP_DIR/ready.fifo" output="$TMP_DIR/event.json"
  rm -f "$ready" "$output"
  mkfifo "$ready"
  python3 "$ROOT/evals/plan-authorized-reporting/event_wait.py" \
    "$(dev_api_url)" "$(dev_token)" agent-report "$marker" "$ready" "$output" 20 &
  EVENT_PID=$!
  if ! read -r -t 10 _ <"$ready"; then
    echo "FAIL subscription_ready: agent-report" >&2
    exit 1
  fi
}

finish_report_subscription() {
  wait "$EVENT_PID"
  EVENT_PID=""
  jq -e '.event_type == "agent-report"' "$TMP_DIR/event.json" >/dev/null
}

assert_refused() {
  local label="$1" tab="$2" session="$3" nonce="$4" token="$5" before after output rc
  before="$(plan_json | jq -c '{strides:(.plan.strides // .strides // []),tasks:[(.plan.tasks // .tasks // [])[] | {id,status,assigned_tab_id,milestones}]}')"
  set +e
  output="$(
    unset CODETERM_PLAN_ID CODETERM_TASK_ID
    CODETERM_SESSION_ID="$session" CODETERM_SESSION_BINDING_NONCE="$nonce" \
      dc --as-tab "$tab" agent report "$token" --plan "$PLAN_ID" --json 2>&1
  )"
  rc=$?
  set -e
  [ "$rc" -ne 0 ] || { echo "FAIL $label: report accepted"; exit 1; }
  [ -n "$output" ] || { echo "FAIL $label: refusal had no actionable output"; exit 1; }
  after="$(plan_json | jq -c '{strides:(.plan.strides // .strides // []),tasks:[(.plan.tasks // .tasks // [])[] | {id,status,assigned_tab_id,milestones}]}')"
  [ "$before" = "$after" ] || { echo "FAIL $label: plan evidence or task state changed"; exit 1; }
  token_absent_everywhere "$token" || { echo "FAIL $label: token persisted or delivered"; exit 1; }
  echo "PASS $label"
}

dev_get /api/health >/dev/null
WORKSPACE_ID="$(capture_json dc workspace create "plan-authorized-reporting" --cwd "$ROOT" --json | jq -r '.id')"
[ -n "$WORKSPACE_ID" ] || { echo "FAIL fixture_workspace"; exit 1; }

OWNER_TAB="$(new_tab "reporting-owner")"
OWNER_NONCE="$(bind_tab "$OWNER_TAB" "$OWNER_SESSION")"
CODETERM_SESSION_ID="$OWNER_SESSION" CODETERM_SESSION_BINDING_NONCE="$OWNER_NONCE" \
  dc --as-tab "$OWNER_TAB" agent set-orchestrator --json >/dev/null
PLAN_ID="$(
  CODETERM_SESSION_ID="$OWNER_SESSION" CODETERM_SESSION_BINDING_NONCE="$OWNER_NONCE" \
    capture_json dc --as-tab "$OWNER_TAB" plan create \
      --title "plan-authorized-reporting" --project "$ROOT" --json | plan_id_from_create
)"
[ -n "$PLAN_ID" ] || { echo "FAIL fixture_plan"; exit 1; }

QA_TAB="$(new_tab "authorized-canonical-qa")"
QA_SESSION="qa-session-c307b1"
QA_NONCE="$(bind_tab "$QA_TAB" "$QA_SESSION")"
FOREIGN_TAB="$(new_tab "foreign-reviewer")"
FOREIGN_SESSION="foreign-session-a4d8e2"
FOREIGN_NONCE="$(bind_tab "$FOREIGN_TAB" "$FOREIGN_SESSION")"
UNBOUND_TAB="$(new_tab "unbound-reviewer")"
UNBOUND_NONCE="$(capture_json dc tab inspect --tab "$UNBOUND_TAB" --json | tab_nonce)"
BOUND_TAB="$(new_tab "task-bound-reviewer")"
BOUND_SESSION="bound-session-89a7c5"
BOUND_NONCE="$(bind_tab "$BOUND_TAB" "$BOUND_SESSION")"

CODETERM_SESSION_ID="$OWNER_SESSION" CODETERM_SESSION_BINDING_NONCE="$OWNER_NONCE" \
  dc --as-tab "$OWNER_TAB" plan review authorize-reporter "$PLAN_ID" "$QA_TAB" \
    --class canonical_qa --json >/dev/null
CODETERM_SESSION_ID="$OWNER_SESSION" CODETERM_SESSION_BINDING_NONCE="$OWNER_NONCE" \
  dc --as-tab "$OWNER_TAB" plan review authorize-reporter "$PLAN_ID" "$BOUND_TAB" \
    --class review --json >/dev/null

TASK_ID="$(
  CODETERM_SESSION_ID="$OWNER_SESSION" CODETERM_SESSION_BINDING_NONCE="$OWNER_NONCE" \
    capture_json dc --as-tab "$OWNER_TAB" plan task add "$PLAN_ID" \
      --name "task-bound misuse sentinel" --executor-role "oracle-worker" \
      --role-guidance "opaque-fixture-7ea61b" --criterion "opaque-criterion-c1fb35" \
      --test-plan "opaque-test-5e4d29" --assign-tab "$BOUND_TAB" --json |
    jq -r '.task.id // .id // empty'
)"
[ -n "$TASK_ID" ] || { echo "FAIL fixture_task"; exit 1; }
TASK_BEFORE="$(plan_json | jq -c --arg task "$TASK_ID" '(.plan.tasks // .tasks // [])[] | select(.id == $task) | {status,assigned_tab_id,milestones}')"

ACCEPTED_TOKEN="qa-evidence-4c95a037"
start_report_subscription "$ACCEPTED_TOKEN"
ACCEPTED="$(
  unset CODETERM_PLAN_ID CODETERM_TASK_ID
  CODETERM_SESSION_ID="$QA_SESSION" CODETERM_SESSION_BINDING_NONCE="$QA_NONCE" \
    dc --as-tab "$QA_TAB" agent report "$ACCEPTED_TOKEN" --plan "$PLAN_ID" --json
)"
finish_report_subscription
jq -e --arg plan "$PLAN_ID" \
  '.accepted == true and .plan_report.persisted == true and .plan_report.plan_id == $plan and .plan_report.task_id == null and .plan_report.task_transition == false' \
  >/dev/null <<<"$ACCEPTED"
plan_json | jq -e --arg token "$ACCEPTED_TOKEN" \
  '(.plan.strides // .strides) | any(.kind == "plan_evidence_report" and .text == $token and .from.tab_id != null)' \
  >/dev/null
dev_get "/api/tabs/$OWNER_TAB/reports" | jq -e --arg token "$ACCEPTED_TOKEN" \
  'any(.message == $token and .task_id == null and .status == null)' >/dev/null
TASK_AFTER="$(plan_json | jq -c --arg task "$TASK_ID" '(.plan.tasks // .tasks // [])[] | select(.id == $task) | {status,assigned_tab_id,milestones}')"
[ "$TASK_BEFORE" = "$TASK_AFTER" ] || { echo "FAIL authorized_qa_task_non_mutation"; exit 1; }
echo "PASS authorized_qa_persistence_and_delivery"

assert_refused foreign_tab "$FOREIGN_TAB" "$FOREIGN_SESSION" "$FOREIGN_NONCE" "foreign-refused-0ef21c"
assert_refused unbound_tab "$UNBOUND_TAB" "unbound-session-82ba13" "$UNBOUND_NONCE" "unbound-refused-c306d8"
assert_refused stale_rebound_identity "$QA_TAB" "stale-session-b127ea" "stale-nonce-2e81f9" "stale-refused-95b1ac"
assert_refused task_bound_misuse "$BOUND_TAB" "$BOUND_SESSION" "$BOUND_NONCE" "task-bound-refused-89e40d"

echo "PASS plan_authorized_reporting"
