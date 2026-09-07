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
RUN_ID="${EXPECTED_SHA:0:8}-$$"
PROVIDER_ID="provider-setup-fixture-$RUN_ID"
INSTALL_TOKEN="${PROVIDER_SETUP_TASK_TOKEN}-install"
FAILURE_TOKEN="${PROVIDER_SETUP_TASK_TOKEN}-dispatch-refusal"
FIXTURE_DIR="$ROOT/target-check/evals/provider-setup-lifecycle/$RUN_ID/plugin"
WORKSPACE=""
CONFIGURE_TAB=""
INSTALL_TAB=""
ORIGINAL_PROVIDER=""
RECEIVER_PROCESS=""
CURRENT_ROW="hook contract/refusal and whole-file non-mutation"

cleanup() {
  [ -z "$INSTALL_TAB" ] || dc tab close "$INSTALL_TAB" >/dev/null 2>&1 || true
  [ -z "$CONFIGURE_TAB" ] || dc tab close "$CONFIGURE_TAB" >/dev/null 2>&1 || true
  [ -z "$WORKSPACE" ] || dc workspace close "$WORKSPACE" --force >/dev/null 2>&1 || true
  if [ -n "$ORIGINAL_PROVIDER" ]; then
    original_json="$(node -e 'process.stdout.write(JSON.stringify(process.argv[1]))' "$ORIGINAL_PROVIDER")"
    dc config set general_agent.agent_provider "$original_json" >/dev/null 2>&1 || true
  fi
  dc plugin remove "$PROVIDER_ID" >/dev/null 2>&1 || true
  [ -z "$RECEIVER_PROCESS" ] || kill "$RECEIVER_PROCESS" >/dev/null 2>&1 || true
  node -e 'require("node:fs").rmSync(process.argv[1], { recursive: true, force: true })' "${FIXTURE_DIR%/plugin}" >/dev/null 2>&1 || true
}
trap cleanup EXIT

fail() { printf 'FAIL %s: %s\n' "$CURRENT_ROW" "$1" >&2; exit 1; }
pass() { printf 'PASS %s\n' "$1"; }
field() { node -e 'const v=JSON.parse(require("node:fs").readFileSync(0,"utf8")); const p=process.argv[1].split("."); let x=v; for(const k of p)x=x?.[k]; if(x!=null)process.stdout.write(String(x));' "$1"; }
hook_digest() {
  node -e 'const fs=require("node:fs"),c=require("node:crypto"),p=process.argv[1]; process.stdout.write(fs.existsSync(p)?c.createHash("sha256").update(fs.readFileSync(p)).digest("hex"):"absent")' "$HOME/.codex/hooks.json"
}
read_receipt() {
  local expected_kind="$1" expected_token="$2" marker kind encoded
  IFS=' ' read -r -t 45 marker kind encoded <&"$RECEIVER_FD" || fail "$expected_kind receiver deadline"
  [ "$marker" = OBSERVED ] && [ "$kind" = "$expected_kind" ] || fail "$expected_kind receiver event"
  RECEIPT_TAB="$(printf '%s' "$encoded" | node "$STATE" receipt64 "$expected_token")" || fail "$expected_kind exact-token receipt"
}

HEALTH="$(dev_get /api/health)" || exit 7
printf '%s' "$HEALTH" | node "$STATE" health-sha "$EXPECTED_SHA"
dc identify --json >/dev/null || exit 7

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
coproc RECEIVER { node "$ROOT/evals/provider-setup-lifecycle/receiver.mjs"; }
RECEIVER_PROCESS="$RECEIVER_PID"
exec {RECEIVER_FD}<&"${RECEIVER[0]}"
IFS=' ' read -r RECEIVER_READY COLLECTOR_URL <&"$RECEIVER_FD" || fail "receiver startup"
[ "$RECEIVER_READY" = READY ] && [ -n "$COLLECTOR_URL" ] || fail "receiver readiness"
node "$ROOT/evals/provider-setup-lifecycle/prepare-fixture.mjs" "$FIXTURE_DIR" "$COLLECTOR_URL" "$INSTALL_TOKEN" "$PROVIDER_ID"

coproc PROVIDER_WAIT { dev_run node "$WAIT" provider "$PROVIDER_ID"; }
PROVIDER_WAIT_PROCESS="$PROVIDER_WAIT_PID"
read -r ready <&"${PROVIDER_WAIT[0]}"
[ "$ready" = READY ] || fail "provider observer readiness"
dc plugin install --from-dir "$FIXTURE_DIR" --json >/dev/null || fail "dev-local fixture provider install"
read -r PROVIDER_OBSERVED <&"${PROVIDER_WAIT[0]}"
wait "$PROVIDER_WAIT_PROCESS"
[ "$(printf '%s' "$PROVIDER_OBSERVED" | field provider)" = "$PROVIDER_ID" ] || fail "fixture provider contribution event"

ORIGINAL_PROVIDER_JSON="$(dc --json config get general_agent.agent_provider)"
ORIGINAL_PROVIDER="$(printf '%s' "$ORIGINAL_PROVIDER_JSON" | node "$STATE" config)"
PROVIDER_JSON="$(node -e 'process.stdout.write(JSON.stringify(process.argv[1]))' "$PROVIDER_ID")"
dc config set general_agent.agent_provider "$PROVIDER_JSON" >/dev/null

WORKSPACE_JSON="$(dc --json workspace create "provider-setup-$RUN_ID" --cwd "$ROOT")"
WORKSPACE="$(printf '%s' "$WORKSPACE_JSON" | field id)"
[ -n "$WORKSPACE" ] || WORKSPACE="$(printf '%s' "$WORKSPACE_JSON" | field workspace_id)"
[ -n "$WORKSPACE" ] || fail "fixture workspace creation"

CONFIGURE_TITLE="Configure-fixture-$RUN_ID"
coproc CONFIGURE_WAIT { dev_run node "$WAIT" agent-ready "$CONFIGURE_TITLE" "$PROVIDER_ID"; }
CONFIGURE_WAIT_PROCESS="$CONFIGURE_WAIT_PID"
read -r ready <&"${CONFIGURE_WAIT[0]}"
[ "$ready" = READY ] || fail "Configure observer readiness"
SPAWN_BODY="$(node -e 'process.stdout.write(JSON.stringify({title:process.argv[1],workspace_id:process.argv[2],task:process.argv[3],require_delivery_ack:true,activate:false,audience:"automation"}))' "$CONFIGURE_TITLE" "$WORKSPACE" "$PROVIDER_SETUP_TASK_TOKEN")"
SPAWN_RESULT="$(dev_post /api/agents/spawn "$SPAWN_BODY")" || fail "Configure spawn route"
printf '%s' "$SPAWN_RESULT" | node "$STATE" spawn "$PROVIDER_ID" || fail "configured General-Agent selection and delivery acknowledgement"
CONFIGURE_TAB="$(printf '%s' "$SPAWN_RESULT" | field tab_id)"
read -r CONFIGURE_OBSERVED <&"${CONFIGURE_WAIT[0]}"
wait "$CONFIGURE_WAIT_PROCESS"
[ "$(printf '%s' "$CONFIGURE_OBSERVED" | field tab_id)" = "$CONFIGURE_TAB" ] || fail "Configure AgentReady state"
read_receipt configure "$PROVIDER_SETUP_TASK_TOKEN"
[ "$RECEIPT_TAB" = "$CONFIGURE_TAB" ] || fail "Configure token receiver tab identity"

MISSING_PROVIDER="provider-setup-missing-$RUN_ID"
MISSING_JSON="$(node -e 'process.stdout.write(JSON.stringify(process.argv[1]))' "$MISSING_PROVIDER")"
dc config set general_agent.agent_provider "$MISSING_JSON" >/dev/null
FAIL_TITLE="Configure-failure-$RUN_ID"
FAIL_BODY="$(node -e 'process.stdout.write(JSON.stringify({title:process.argv[1],workspace_id:process.argv[2],task:process.argv[3],require_delivery_ack:true,activate:false,audience:"automation"}))' "$FAIL_TITLE" "$WORKSPACE" "$FAILURE_TOKEN")"
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
INSTALL_TITLE="Install $PROVIDER_ID"
coproc INSTALL_WAIT { dev_run node "$WAIT" install-live "$INSTALL_TITLE"; }
INSTALL_WAIT_PROCESS="$INSTALL_WAIT_PID"
read -r ready <&"${INSTALL_WAIT[0]}"
[ "$ready" = READY ] || fail "installer observer readiness"
INSTALL_RESULT="$(dev_post "/api/agents/providers/$PROVIDER_ID/install-in-tab" '{}')" || fail "installer route"
printf '%s' "$INSTALL_RESULT" | node "$STATE" install || fail "installer dispatch acknowledgement"
INSTALL_TAB="$(printf '%s' "$INSTALL_RESULT" | field tab_id)"
read -r INSTALL_OBSERVED <&"${INSTALL_WAIT[0]}"
wait "$INSTALL_WAIT_PROCESS"
[ "$(printf '%s' "$INSTALL_OBSERVED" | field tab_id)" = "$INSTALL_TAB" ] || fail "installer live tab/PTY state"
read_receipt install "$INSTALL_TOKEN"
[ "$RECEIPT_TAB" = "$INSTALL_TAB" ] || fail "installer command receiver tab identity"
dc tab close "$INSTALL_TAB" >/dev/null
INSTALL_TAB=""

ENCODED_FAILURE="$(node -e 'process.stdout.write(encodeURIComponent(process.argv[1]))' "$FAILURE_TOKEN")"
coproc ROLLBACK_WAIT { dev_run node "$WAIT" install-rollback "$INSTALL_TITLE"; }
ROLLBACK_WAIT_PROCESS="$ROLLBACK_WAIT_PID"
read -r ready <&"${ROLLBACK_WAIT[0]}"
[ "$ready" = READY ] || fail "installer rollback observer readiness"
set +e
INSTALL_FAILURE="$(dev_post "/api/agents/providers/$PROVIDER_ID/install-in-tab?fixture_dispatch_failure=$ENCODED_FAILURE" '{}' 2>&1)"
INSTALL_FAILURE_RC=$?
set -e
[ "$INSTALL_FAILURE_RC" -eq 22 ] && printf '%s' "$INSTALL_FAILURE" | grep -Fq "$FAILURE_TOKEN" || fail "installer exact dispatch refusal"
read -r ROLLBACK_OBSERVED <&"${ROLLBACK_WAIT[0]}"
wait "$ROLLBACK_WAIT_PROCESS"
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
