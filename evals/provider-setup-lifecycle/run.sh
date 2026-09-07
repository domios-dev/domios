#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
source "$ROOT/evals/_lib/lib.sh"
STATE="$ROOT/evals/provider-setup-lifecycle/assert-state.mjs"
WAIT="$ROOT/evals/provider-setup-lifecycle/wait-state.mjs"

if [ "${1:-}" != "--check" ]; then
  echo "usage: PROVIDER_SETUP_TASK_TOKEN=<opaque> $0 --check" >&2
  exit 2
fi
: "${PROVIDER_SETUP_TASK_TOKEN:?supply an opaque PROVIDER_SETUP_TASK_TOKEN}"

EXPECTED_SHA="$(git -C "$ROOT" rev-parse HEAD)"
RUN_ID="${EXPECTED_SHA:0:8}-$(node -e 'process.stdout.write(require("node:crypto").randomUUID())')"
PROVIDER_ID="provider-setup-fixture-$RUN_ID"
INSTALL_TOKEN="${PROVIDER_SETUP_TASK_TOKEN}-install-$RUN_ID"
FAILURE_TOKEN="${PROVIDER_SETUP_TASK_TOKEN}-dispatch-refusal-$RUN_ID"
FIXTURE_DIR="$ROOT/target-check/evals/provider-setup-lifecycle/$RUN_ID/plugin"
WORKSPACE_NAME="provider-setup-$RUN_ID"
WORKSPACE_CWD="${FIXTURE_DIR%/plugin}"
if is_windows; then WORKSPACE_CWD="$(cygpath -w "$WORKSPACE_CWD")"; fi
CONFIGURE_TITLE="Configure-fixture-$RUN_ID"
FAIL_TITLE="Configure-failure-$RUN_ID"
INSTALL_TITLE="Install $PROVIDER_ID"
MISSING_PROVIDER="provider-setup-missing-$RUN_ID"
WORKSPACE=""
CONFIGURE_TAB=""
INSTALL_TAB=""
ORIGINAL_PROVIDER=""
RECEIVER_PROCESS=""
RECEIVER_FD=""
PROVIDER_WAIT_PROCESS=""
PROVIDER_WAIT_FD=""
CONFIGURE_WAIT_PROCESS=""
CONFIGURE_WAIT_FD=""
INSTALL_WAIT_PROCESS=""
INSTALL_WAIT_FD=""
ROLLBACK_WAIT_PROCESS=""
ROLLBACK_WAIT_FD=""
FAILED_TAB=""
FAILED_PTY=""
FIXTURE_OWNED=0
PLUGIN_OWNED=0
CONFIG_CHANGED=0
WORKSPACE_OWNED=0
CONFIGURE_TAB_OWNED=0
INSTALL_TAB_OWNED=0
PLUGIN_INSTALL_ATTEMPTED=0
WORKSPACE_CREATE_ATTEMPTED=0
CONFIGURE_SPAWN_ATTEMPTED=0
CONFIGURE_FAILURE_ATTEMPTED=0
INSTALL_LAUNCH_ATTEMPTED=0
ROLLBACK_ATTEMPTED=0
PASS_ROWS=()
CURRENT_ROW="hook contract/refusal and whole-file non-mutation"

cleanup() {
  local status=$? cleanup_failed=0 process row tabs workspaces providers original_json
  set +e
  trap - EXIT

  for process in "$PROVIDER_WAIT_PROCESS" "$CONFIGURE_WAIT_PROCESS" "$INSTALL_WAIT_PROCESS" "$ROLLBACK_WAIT_PROCESS"; do
    if [ -n "$process" ]; then
      [ "$status" -ne 0 ] || { echo "FAIL cleanup: event observer remained active" >&2; cleanup_failed=1; }
      kill "$process" >/dev/null 2>&1
      wait "$process" >/dev/null 2>&1
    fi
  done
  [ -z "$PROVIDER_WAIT_FD" ] || exec {PROVIDER_WAIT_FD}<&-
  [ -z "$CONFIGURE_WAIT_FD" ] || exec {CONFIGURE_WAIT_FD}<&-
  [ -z "$INSTALL_WAIT_FD" ] || exec {INSTALL_WAIT_FD}<&-
  [ -z "$ROLLBACK_WAIT_FD" ] || exec {ROLLBACK_WAIT_FD}<&-

  if [ "$INSTALL_TAB_OWNED" -eq 1 ]; then
    dc tab close "$INSTALL_TAB" >/dev/null 2>&1 || { echo "FAIL cleanup: owned install tab close" >&2; cleanup_failed=1; }
  fi
  if [ "$CONFIGURE_TAB_OWNED" -eq 1 ]; then
    dc tab close "$CONFIGURE_TAB" >/dev/null 2>&1 || { echo "FAIL cleanup: owned Configure tab close" >&2; cleanup_failed=1; }
  fi
  if [ "$INSTALL_LAUNCH_ATTEMPTED" -eq 1 ] || [ "$CONFIGURE_SPAWN_ATTEMPTED" -eq 1 ] || [ "$CONFIGURE_FAILURE_ATTEMPTED" -eq 1 ] || [ "$ROLLBACK_ATTEMPTED" -eq 1 ]; then
    tabs="$(dev_get '/api/tabs?all_workspaces=true')" || { echo "FAIL cleanup: tab residue query" >&2; cleanup_failed=1; tabs=""; }
    [ "$INSTALL_TAB_OWNED" -eq 0 ] || printf '%s' "$tabs" | node "$STATE" no-tab "$INSTALL_TAB" || { echo "FAIL cleanup: install tab residue" >&2; cleanup_failed=1; }
    [ "$CONFIGURE_TAB_OWNED" -eq 0 ] || printf '%s' "$tabs" | node "$STATE" no-tab "$CONFIGURE_TAB" || { echo "FAIL cleanup: Configure tab residue" >&2; cleanup_failed=1; }
    [ "$INSTALL_LAUNCH_ATTEMPTED" -eq 0 ] || printf '%s' "$tabs" | node "$STATE" no-title "$INSTALL_TITLE" || { echo "FAIL cleanup: run-scoped install tab residue" >&2; cleanup_failed=1; }
    [ "$CONFIGURE_SPAWN_ATTEMPTED" -eq 0 ] || printf '%s' "$tabs" | node "$STATE" no-title "$CONFIGURE_TITLE" || { echo "FAIL cleanup: run-scoped Configure tab residue" >&2; cleanup_failed=1; }
    [ "$CONFIGURE_FAILURE_ATTEMPTED" -eq 0 ] || printf '%s' "$tabs" | node "$STATE" no-title "$FAIL_TITLE" || { echo "FAIL cleanup: failed Configure tab residue" >&2; cleanup_failed=1; }
  fi
  if [ -n "$FAILED_TAB" ]; then
    tabs="$(dev_get '/api/tabs?all_workspaces=true')" || { echo "FAIL cleanup: rollback tab residue query" >&2; cleanup_failed=1; tabs=""; }
    printf '%s' "$tabs" | node "$STATE" no-tab "$FAILED_TAB" || { echo "FAIL cleanup: rollback tab residue" >&2; cleanup_failed=1; }
  fi
  if [ -n "$FAILED_PTY" ]; then
    dev_get "/api/pty/$FAILED_PTY/read" >/dev/null 2>&1
    [ $? -eq 22 ] || { echo "FAIL cleanup: rollback PTY residue" >&2; cleanup_failed=1; }
  fi
  if [ "$WORKSPACE_OWNED" -eq 1 ]; then
    dc workspace close "$WORKSPACE" >/dev/null 2>&1 || { echo "FAIL cleanup: owned workspace close" >&2; cleanup_failed=1; }
  fi
  if [ "$WORKSPACE_CREATE_ATTEMPTED" -eq 1 ]; then
    workspaces="$(dev_get '/api/workspaces')" || { echo "FAIL cleanup: workspace residue query" >&2; cleanup_failed=1; workspaces=""; }
    [ "$WORKSPACE_OWNED" -eq 0 ] || printf '%s' "$workspaces" | node "$STATE" no-workspace "$WORKSPACE" || { echo "FAIL cleanup: workspace residue" >&2; cleanup_failed=1; }
    printf '%s' "$workspaces" | node "$STATE" no-workspace-name "$WORKSPACE_NAME" || { echo "FAIL cleanup: run-scoped workspace residue" >&2; cleanup_failed=1; }
  fi
  if [ "$CONFIG_CHANGED" -eq 1 ]; then
    original_json="$(node -e 'process.stdout.write(JSON.stringify(process.argv[1]))' "$ORIGINAL_PROVIDER")"
    dc config set general_agent.agent_provider "$original_json" >/dev/null 2>&1 || { echo "FAIL cleanup: General Agent provider restore" >&2; cleanup_failed=1; }
    dc --json config get general_agent.agent_provider | node "$STATE" config-equals "$ORIGINAL_PROVIDER" || { echo "FAIL cleanup: General Agent provider residue" >&2; cleanup_failed=1; }
  fi
  if [ "$PLUGIN_OWNED" -eq 1 ]; then
    dc plugin remove "$PROVIDER_ID" >/dev/null 2>&1 || { echo "FAIL cleanup: owned fixture provider removal" >&2; cleanup_failed=1; }
  fi
  if [ "$PLUGIN_INSTALL_ATTEMPTED" -eq 1 ]; then
    providers="$(dev_get '/api/agents/providers')" || { echo "FAIL cleanup: provider residue query" >&2; cleanup_failed=1; providers=""; }
    printf '%s' "$providers" | node "$STATE" no-provider "$PROVIDER_ID" || { echo "FAIL cleanup: provider residue" >&2; cleanup_failed=1; }
  fi
  if [ -n "$RECEIVER_PROCESS" ]; then
    kill "$RECEIVER_PROCESS" >/dev/null 2>&1 || { echo "FAIL cleanup: receiver was not alive" >&2; cleanup_failed=1; }
    wait "$RECEIVER_PROCESS" >/dev/null 2>&1
  fi
  [ -z "$RECEIVER_FD" ] || exec {RECEIVER_FD}<&-
  if [ "$FIXTURE_OWNED" -eq 1 ]; then
    node -e 'require("node:fs").rmSync(process.argv[1], { recursive: true, force: true })' "${FIXTURE_DIR%/plugin}" >/dev/null 2>&1 || { echo "FAIL cleanup: fixture directory removal" >&2; cleanup_failed=1; }
    [ ! -e "${FIXTURE_DIR%/plugin}" ] || { echo "FAIL cleanup: fixture directory residue" >&2; cleanup_failed=1; }
  fi
  if [ "$cleanup_failed" -ne 0 ] && [ "$status" -eq 0 ]; then
    status=1
  fi
  if [ "$status" -eq 0 ]; then
    for row in "${PASS_ROWS[@]}"; do
      printf 'PASS %s\n' "$row"
    done
  fi
  exit "$status"
}
trap cleanup EXIT

fail() { printf 'FAIL %s: %s\n' "$CURRENT_ROW" "$1" >&2; exit 1; }
pass() { PASS_ROWS+=("$1"); }
field() { node -e 'const v=JSON.parse(require("node:fs").readFileSync(0,"utf8")); const p=process.argv[1].split("."); let x=v; for(const k of p)x=x?.[k]; if(x!=null)process.stdout.write(String(x));' "$1"; }
hook_digest() {
  node -e 'const fs=require("node:fs"),c=require("node:crypto"),p=process.argv[1]; process.stdout.write(fs.existsSync(p)?c.createHash("sha256").update(fs.readFileSync(p)).digest("hex"):"absent")' "$HOME/.codex/hooks.json"
}
read_receipt() {
  local expected_kind="$1" expected_token="$2" encoded_kind encoded_token receipt
  encoded_kind="$(node -e 'process.stdout.write(encodeURIComponent(process.argv[1]))' "$expected_kind")"
  encoded_token="$(node -e 'process.stdout.write(encodeURIComponent(process.argv[1]))' "$expected_token")"
  receipt="$(curl -sS --fail --max-time 45 "$COLLECTOR_URL/wait?kind=$encoded_kind&token=$encoded_token")" || fail "$expected_kind receiver deadline"
  RECEIPT_TAB="$(printf '%s' "$receipt" | node "$STATE" receipt "$expected_token")" || fail "$expected_kind exact-token receipt"
}

HEALTH="$(dev_get /api/health)" || exit 7
printf '%s' "$HEALTH" | node "$STATE" health-sha "$EXPECTED_SHA"
dc identify --json >/dev/null || exit 7
DEV_API_URL="$(dev_api_url)" || exit 7
DEV_API_TOKEN="$(dev_token)" || exit 7

HOOK_BEFORE="$(hook_digest)"
HOOK_STATUS="$(dev_get '/api/setup/hooks?provider=codex')" || fail "Codex integration status API"
printf '%s' "$HOOK_STATUS" | node "$STATE" hook || fail "Codex full contract or consent-required refusal"
set +e
HOOK_REFUSAL="$(CODETERM_HTTP_STATUS_LOG=1 dev_post '/api/setup/hooks/install?provider=codex' '{}' 2>&1)"
HOOK_REFUSAL_RC=$?
set -e
[ "$HOOK_REFUSAL_RC" -eq 22 ] && printf '%s' "$HOOK_REFUSAL" | grep -q 'HTTP 403' || fail "Codex unconsented global write refusal"
[ "$(hook_digest)" = "$HOOK_BEFORE" ] || fail "Codex hook file changed during read/refusal"
pass "hook contract/refusal and whole-file non-mutation"

CURRENT_ROW="Configure exact-token receipt/failure"
exec {RECEIVER_FD}< <(node "$ROOT/evals/provider-setup-lifecycle/receiver.mjs")
RECEIVER_PROCESS=$!
IFS=' ' read -r -t 10 RECEIVER_READY COLLECTOR_URL <&"$RECEIVER_FD" || fail "receiver startup"
[ "$RECEIVER_READY" = READY ] && [ -n "$COLLECTOR_URL" ] || fail "receiver readiness"
exec {RECEIVER_FD}<&-
RECEIVER_FD=""
[ ! -e "${FIXTURE_DIR%/plugin}" ] || fail "unique fixture directory already exists"
FIXTURE_OWNED=1
node "$ROOT/evals/provider-setup-lifecycle/prepare-fixture.mjs" "$FIXTURE_DIR" "$COLLECTOR_URL" "$INSTALL_TOKEN" "$PROVIDER_ID"

exec {PROVIDER_WAIT_FD}< <(node "$WAIT" "$DEV_API_URL" "$DEV_API_TOKEN" provider "$PROVIDER_ID")
PROVIDER_WAIT_PROCESS=$!
read -r -t 10 ready <&"$PROVIDER_WAIT_FD" || fail "provider observer startup"
[ "$ready" = READY ] || fail "provider observer readiness"
printf '%s' "$(dev_get '/api/agents/providers')" | node "$STATE" no-provider "$PROVIDER_ID" || fail "unique fixture provider id was already present"
PLUGIN_INSTALL_ATTEMPTED=1
dc plugin install --from-dir "$FIXTURE_DIR" --json >/dev/null || fail "dev-local fixture provider install"
PLUGIN_OWNED=1
read -r -t 45 PROVIDER_OBSERVED <&"$PROVIDER_WAIT_FD" || fail "provider observer deadline"
if ! wait "$PROVIDER_WAIT_PROCESS"; then PROVIDER_WAIT_PROCESS=""; fail "provider observer failure"; fi
PROVIDER_WAIT_PROCESS=""
exec {PROVIDER_WAIT_FD}<&-
PROVIDER_WAIT_FD=""
[ "$(printf '%s' "$PROVIDER_OBSERVED" | field provider)" = "$PROVIDER_ID" ] || fail "fixture provider contribution event"

ORIGINAL_PROVIDER_JSON="$(dc --json config get general_agent.agent_provider)"
ORIGINAL_PROVIDER="$(printf '%s' "$ORIGINAL_PROVIDER_JSON" | node "$STATE" config)"
PROVIDER_JSON="$(node -e 'process.stdout.write(JSON.stringify(process.argv[1]))' "$PROVIDER_ID")"
CONFIG_CHANGED=1
dc config set general_agent.agent_provider "$PROVIDER_JSON" >/dev/null

WORKSPACE_CREATE_ATTEMPTED=1
WORKSPACES_BEFORE="$(dev_get '/api/workspaces')"
printf '%s' "$WORKSPACES_BEFORE" | node "$STATE" no-workspace-name "$WORKSPACE_NAME" || fail "fixture workspace name already exists"
printf '%s' "$WORKSPACES_BEFORE" | node "$STATE" no-workspace-cwd "$WORKSPACE_CWD" || fail "fixture workspace cwd already exists"
WORKSPACE_JSON="$(dc --json workspace create "$WORKSPACE_NAME" --cwd "$WORKSPACE_CWD")"
WORKSPACE="$(printf '%s' "$WORKSPACE_JSON" | node "$STATE" workspace-created "$WORKSPACE_NAME" "$WORKSPACE_CWD")" || fail "fixture workspace creation"
WORKSPACE_OWNED=1

exec {CONFIGURE_WAIT_FD}< <(node "$WAIT" "$DEV_API_URL" "$DEV_API_TOKEN" agent-ready "$CONFIGURE_TITLE" "$PROVIDER_ID")
CONFIGURE_WAIT_PROCESS=$!
read -r -t 10 ready <&"$CONFIGURE_WAIT_FD" || fail "Configure observer startup"
[ "$ready" = READY ] || fail "Configure observer readiness"
SPAWN_BODY="$(node -e 'process.stdout.write(JSON.stringify({title:process.argv[1],workspace_id:process.argv[2],task:process.argv[3],require_delivery_ack:true,activate:false,audience:"automation"}))' "$CONFIGURE_TITLE" "$WORKSPACE" "$PROVIDER_SETUP_TASK_TOKEN")"
CONFIGURE_SPAWN_ATTEMPTED=1
SPAWN_RESULT="$(dev_post /api/agents/spawn "$SPAWN_BODY")" || fail "Configure spawn route"
printf '%s' "$SPAWN_RESULT" | node "$STATE" spawn "$PROVIDER_ID" || fail "configured General-Agent selection and delivery acknowledgement"
CONFIGURE_TAB="$(printf '%s' "$SPAWN_RESULT" | field tab_id)"
[ -n "$CONFIGURE_TAB" ] || fail "Configure spawn tab identity"
CONFIGURE_TAB_OWNED=1
read -r -t 45 CONFIGURE_OBSERVED <&"$CONFIGURE_WAIT_FD" || fail "Configure observer deadline"
if ! wait "$CONFIGURE_WAIT_PROCESS"; then CONFIGURE_WAIT_PROCESS=""; fail "Configure observer failure"; fi
CONFIGURE_WAIT_PROCESS=""
exec {CONFIGURE_WAIT_FD}<&-
CONFIGURE_WAIT_FD=""
[ "$(printf '%s' "$CONFIGURE_OBSERVED" | field tab_id)" = "$CONFIGURE_TAB" ] || fail "Configure AgentReady state"
read_receipt configure "$PROVIDER_SETUP_TASK_TOKEN"
[ "$RECEIPT_TAB" = "$CONFIGURE_TAB" ] || fail "Configure token receiver tab identity"

MISSING_JSON="$(node -e 'process.stdout.write(JSON.stringify(process.argv[1]))' "$MISSING_PROVIDER")"
dc config set general_agent.agent_provider "$MISSING_JSON" >/dev/null
FAIL_BODY="$(node -e 'process.stdout.write(JSON.stringify({title:process.argv[1],workspace_id:process.argv[2],task:process.argv[3],require_delivery_ack:true,activate:false,audience:"automation"}))' "$FAIL_TITLE" "$WORKSPACE" "$FAILURE_TOKEN")"
CONFIGURE_FAILURE_ATTEMPTED=1
set +e
CONFIGURE_FAILURE="$(dev_post /api/agents/spawn "$FAIL_BODY" 2>&1)"
CONFIGURE_FAILURE_RC=$?
set -e
[ "$CONFIGURE_FAILURE_RC" -eq 22 ] && printf '%s' "$CONFIGURE_FAILURE" | grep -Fq "$MISSING_PROVIDER" || fail "Configure exact launch refusal"
TABS_AFTER_CONFIGURE_FAILURE="$(dev_get '/api/tabs?all_workspaces=true')"
printf '%s' "$TABS_AFTER_CONFIGURE_FAILURE" | node "$STATE" no-title "$FAIL_TITLE" || fail "Configure failure rollback"
dc config set general_agent.agent_provider "$PROVIDER_JSON" >/dev/null
pass "Configure exact-token receipt and exact failure without focus-only acceptance"

CURRENT_ROW="Install Provider launch/failure"
exec {INSTALL_WAIT_FD}< <(node "$WAIT" "$DEV_API_URL" "$DEV_API_TOKEN" install-live "$INSTALL_TITLE")
INSTALL_WAIT_PROCESS=$!
read -r -t 10 ready <&"$INSTALL_WAIT_FD" || fail "installer observer startup"
[ "$ready" = READY ] || fail "installer observer readiness"
INSTALL_LAUNCH_ATTEMPTED=1
INSTALL_RESULT="$(dev_post "/api/agents/providers/$PROVIDER_ID/install-in-tab" '{}')" || fail "installer route"
printf '%s' "$INSTALL_RESULT" | node "$STATE" install || fail "installer dispatch acknowledgement"
INSTALL_TAB="$(printf '%s' "$INSTALL_RESULT" | field tab_id)"
[ -n "$INSTALL_TAB" ] || fail "installer tab identity"
INSTALL_TAB_OWNED=1
read -r -t 45 INSTALL_OBSERVED <&"$INSTALL_WAIT_FD" || fail "installer observer deadline"
if ! wait "$INSTALL_WAIT_PROCESS"; then INSTALL_WAIT_PROCESS=""; fail "installer observer failure"; fi
INSTALL_WAIT_PROCESS=""
exec {INSTALL_WAIT_FD}<&-
INSTALL_WAIT_FD=""
[ "$(printf '%s' "$INSTALL_OBSERVED" | field tab_id)" = "$INSTALL_TAB" ] || fail "installer live tab/PTY state"
read_receipt install "$INSTALL_TOKEN"
[ "$RECEIPT_TAB" = "$INSTALL_TAB" ] || fail "installer command receiver tab identity"
dc tab close "$INSTALL_TAB" >/dev/null
printf '%s' "$(dev_get '/api/tabs?all_workspaces=true')" | node "$STATE" no-tab "$INSTALL_TAB" || fail "installer owned tab cleanup"
INSTALL_TAB=""
INSTALL_TAB_OWNED=0

ENCODED_FAILURE="$(node -e 'process.stdout.write(encodeURIComponent(process.argv[1]))' "$FAILURE_TOKEN")"
exec {ROLLBACK_WAIT_FD}< <(node "$WAIT" "$DEV_API_URL" "$DEV_API_TOKEN" install-rollback "$INSTALL_TITLE")
ROLLBACK_WAIT_PROCESS=$!
read -r -t 10 ready <&"$ROLLBACK_WAIT_FD" || fail "installer rollback observer startup"
[ "$ready" = READY ] || fail "installer rollback observer readiness"
ROLLBACK_ATTEMPTED=1
set +e
INSTALL_FAILURE="$(dev_post "/api/agents/providers/$PROVIDER_ID/install-in-tab?fixture_dispatch_failure=$ENCODED_FAILURE" '{}' 2>&1)"
INSTALL_FAILURE_RC=$?
set -e
[ "$INSTALL_FAILURE_RC" -eq 22 ] && printf '%s' "$INSTALL_FAILURE" | grep -Fq "$FAILURE_TOKEN" || fail "installer exact dispatch refusal"
read -r -t 45 ROLLBACK_OBSERVED <&"$ROLLBACK_WAIT_FD" || fail "installer rollback observer deadline"
if ! wait "$ROLLBACK_WAIT_PROCESS"; then ROLLBACK_WAIT_PROCESS=""; fail "installer rollback observer failure"; fi
ROLLBACK_WAIT_PROCESS=""
exec {ROLLBACK_WAIT_FD}<&-
ROLLBACK_WAIT_FD=""
FAILED_TAB="$(printf '%s' "$ROLLBACK_OBSERVED" | field tab_id)"
FAILED_PTY="$(printf '%s' "$ROLLBACK_OBSERVED" | field pty_id)"
[ -n "$FAILED_TAB" ] && [ -n "$FAILED_PTY" ] || fail "installer rollback identities"
printf '%s' "$(dev_get '/api/tabs?all_workspaces=true')" | node "$STATE" no-title "$INSTALL_TITLE" || fail "installer provisional tab cleanup"
set +e
PTY_AFTER_ROLLBACK="$(dev_get "/api/pty/$FAILED_PTY/read" 2>&1)"
PTY_AFTER_ROLLBACK_RC=$?
set -e
[ "$PTY_AFTER_ROLLBACK_RC" -eq 22 ] || fail "installer provisional PTY cleanup"
pass "Install Provider executed command or returned exact refusal with tab/PTY rollback"
