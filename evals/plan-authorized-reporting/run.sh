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
JSON="$ROOT/evals/plan-authorized-reporting/json.mjs"

cleanup() {
  local status=$?
  [ -z "$EVENT_PID" ] || kill "$EVENT_PID" >/dev/null 2>&1 || true
  if [ -n "$PLAN_ID" ] && [ -n "$OWNER_TAB" ] && [ -n "$OWNER_NONCE" ]; then
    CODETERM_SESSION_ID="$OWNER_SESSION" CODETERM_SESSION_BINDING_NONCE="$OWNER_NONCE" \
      dc --as-tab "$OWNER_TAB" plan delete "$PLAN_ID" --yes >/dev/null 2>&1 || true
  fi
  [ -z "$WORKSPACE_ID" ] || dc workspace close "$WORKSPACE_ID" >/dev/null 2>&1 || true
  rm -rf "$TMP_DIR"
  trap - EXIT
  exit "$status"
}
trap cleanup EXIT

capture_json() {
  local output
  output="$("$@")"
  printf '%s\n' "$output"
}

tab_id_from_create() {
  node "$JSON" tab-id
}

plan_id_from_create() {
  node "$JSON" plan-id
}

tab_nonce() {
  node "$JSON" tab-nonce
}

bind_tab() {
  local tab_id="$1" session_id="$2" nonce body response
  nonce="$(capture_json dc tab inspect --tab "$tab_id" --json | tab_nonce)"
  [ -n "$nonce" ] || { echo "FAIL fixture_binding: missing nonce for $tab_id"; exit 1; }
  body="$(node "$JSON" session-body "$session_id" "$nonce" </dev/null)"
  response="$(dev_post "/api/tabs/$tab_id/hook/session_start" "$body")"
  node "$JSON" bind-ok "$session_id" <<<"$response"
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
  plan_json | node "$JSON" token-absent "$token"
  dev_get "/api/tabs/$OWNER_TAB/reports" | node "$JSON" report-token-absent "$token"
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
  node "$JSON" event-type <"$TMP_DIR/event.json"
}

assert_refused() {
  local label="$1" tab="$2" session="$3" nonce="$4" token="$5" before after output rc
  before="$(plan_json | node "$JSON" plan-snapshot)"
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
  after="$(plan_json | node "$JSON" plan-snapshot)"
  [ "$before" = "$after" ] || { echo "FAIL $label: plan evidence or task state changed"; exit 1; }
  token_absent_everywhere "$token" || { echo "FAIL $label: token persisted or delivered"; exit 1; }
  echo "PASS $label"
}

dev_get /api/health >/dev/null
WORKSPACE_ID="$(capture_json dc workspace create "plan-authorized-reporting" --cwd "$ROOT" --json | node "$JSON" tab-id)"
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
    node "$JSON" task-id
)"
[ -n "$TASK_ID" ] || { echo "FAIL fixture_task"; exit 1; }
TASK_BEFORE="$(plan_json | node "$JSON" task-snapshot "$TASK_ID")"

ACCEPTED_TOKEN="qa-evidence-4c95a037"
start_report_subscription "$ACCEPTED_TOKEN"
ACCEPTED="$(
  unset CODETERM_PLAN_ID CODETERM_TASK_ID
  CODETERM_SESSION_ID="$QA_SESSION" CODETERM_SESSION_BINDING_NONCE="$QA_NONCE" \
    dc --as-tab "$QA_TAB" agent report "$ACCEPTED_TOKEN" --plan "$PLAN_ID" --json
)"
finish_report_subscription
node "$JSON" accepted-response "$PLAN_ID" <<<"$ACCEPTED"
plan_json | node "$JSON" plan-evidence "$ACCEPTED_TOKEN"
dev_get "/api/tabs/$OWNER_TAB/reports" | node "$JSON" owner-report "$ACCEPTED_TOKEN"
TASK_AFTER="$(plan_json | node "$JSON" task-snapshot "$TASK_ID")"
[ "$TASK_BEFORE" = "$TASK_AFTER" ] || { echo "FAIL authorized_qa_task_non_mutation"; exit 1; }
echo "PASS authorized_qa_persistence_and_delivery"

assert_refused foreign_tab "$FOREIGN_TAB" "$FOREIGN_SESSION" "$FOREIGN_NONCE" "foreign-refused-0ef21c"
assert_refused unbound_tab "$UNBOUND_TAB" "unbound-session-82ba13" "$UNBOUND_NONCE" "unbound-refused-c306d8"
assert_refused stale_rebound_identity "$QA_TAB" "stale-session-b127ea" "stale-nonce-2e81f9" "stale-refused-95b1ac"
assert_refused task_bound_misuse "$BOUND_TAB" "$BOUND_SESSION" "$BOUND_NONCE" "task-bound-refused-89e40d"

echo "PASS plan_authorized_reporting"
