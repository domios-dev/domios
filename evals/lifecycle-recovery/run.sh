#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
source "$ROOT/evals/_lib/lib.sh"
JSON_STATE="$ROOT/evals/lifecycle-recovery/json-state.mjs"

fail() { echo "FAIL $1" >&2; exit 1; }
pass() { echo "PASS $1"; }
json_field() { node "$JSON_STATE" field "$2" <<<"$1"; }

HEALTH="$(dev_get /api/health)"
EXPECTED_SHA="$(git -C "$ROOT" rev-parse HEAD)"
[ "$(json_field "$HEALTH" '.build_sha')" = "$EXPECTED_SHA" ] || fail "build SHA mismatch"
CONTROL_TABS="$(dev_get '/api/tabs?all_workspaces=true')"
CONTROL_TAB="$(node "$JSON_STATE" general-agent <<<"$CONTROL_TABS")"
[ -n "$CONTROL_TAB" ] || fail "external cleanup caller"

RUN_ID="${EXPECTED_SHA:0:8}-$$"
WORKSPACE=""
EXIT_TAB=""
EXIT_TEAM_ID=""
EXIT_REPLACEMENT=""
RELAUNCH_TAB=""
OLD_ORCHESTRATOR=""
REPLACEMENT=""
TEAM_ID=""
cleanup() {
  [ -z "$TEAM_ID" ] || [ -z "$REPLACEMENT" ] || dc --as-tab "$REPLACEMENT" team close "$TEAM_ID" --json >/dev/null 2>&1 || true
  [ -z "$EXIT_TEAM_ID" ] || [ -z "$EXIT_REPLACEMENT" ] || dc --as-tab "$EXIT_REPLACEMENT" team close "$EXIT_TEAM_ID" --json >/dev/null 2>&1 || true
  for tab in "$EXIT_TAB" "$RELAUNCH_TAB" "$OLD_ORCHESTRATOR"; do
    [ -z "$tab" ] || dc --as-tab "$CONTROL_TAB" tab close "$tab" >/dev/null 2>&1 || true
  done
  [ -z "$WORKSPACE" ] || dc --as-tab "$CONTROL_TAB" workspace close "$WORKSPACE" >/dev/null 2>&1 || true
}
on_exit() {
  local status=$?
  trap - EXIT
  cleanup
  exit "$status"
}
trap on_exit EXIT

WORKSPACE_JSON="$(dc --json workspace create "lifecycle-recovery-$RUN_ID")"
WORKSPACE="$(json_field "$WORKSPACE_JSON" '.id // .workspace_id')"
[ -n "$WORKSPACE" ] && [ "$WORKSPACE" != null ] || fail "workspace creation"

EXIT_TITLE="LR-exit-$RUN_ID"
coproc EXIT_WAIT { dev_run node "$ROOT/evals/lifecycle-recovery/wait-state.mjs" absent "$EXIT_TITLE"; }
EXIT_WAIT_PROCESS="$EXIT_WAIT_PID"
read -r ready <&"${EXIT_WAIT[0]}"
[ "$ready" = READY ] || fail "exit observer readiness"
EXIT_SPAWN="$(dc --json agent spawn claude --workspace "$WORKSPACE" --title "$EXIT_TITLE" --role Orchestrator --non-user-facing --args --lifecycle-exit-fixture)"
EXIT_TAB="$(json_field "$EXIT_SPAWN" '.tab_id')"
read -r EXIT_OBSERVED <&"${EXIT_WAIT[0]}"
wait "$EXIT_WAIT_PROCESS"
[ "$(json_field "$EXIT_OBSERVED" '.tab_id')" = "$EXIT_TAB" ] || fail "exit observation targeted spawned tab"
EXIT_TEAM_ID="$(json_field "$EXIT_OBSERVED" '.team_id')"
[ -n "$EXIT_TEAM_ID" ] && [ "$EXIT_TEAM_ID" != null ] || fail "exited Orchestrator retained Team identity"
EXIT_VISIBLE="$(dc --json tab list --all-workspaces)"
node "$JSON_STATE" absent-orchestrator "$EXIT_TAB" "$EXIT_TEAM_ID" <<<"$EXIT_VISIBLE" \
  || fail "process-exited tab is CLI-visible as absent"
set +e
RESTART_OUTPUT="$(dc --json agent restart --tab "$EXIT_TAB" 2>&1)"
RESTART_RC=$?
set -e
[ "$RESTART_RC" -ne 0 ] || fail "absent process was advertised as restartable: $RESTART_OUTPUT"
coproc EXIT_RECOVERY_WAIT { dev_run node "$ROOT/evals/lifecycle-recovery/wait-state.mjs" team-owner "$EXIT_TEAM_ID"; }
EXIT_RECOVERY_WAIT_PROCESS="$EXIT_RECOVERY_WAIT_PID"
read -r ready <&"${EXIT_RECOVERY_WAIT[0]}"
[ "$ready" = READY ] || fail "exited-shell replacement observer readiness"
EXIT_REPLACEMENT_SPAWN="$(dc --json agent spawn claude --team "$EXIT_TEAM_ID" --role Orchestrator)"
EXIT_REPLACEMENT="$(json_field "$EXIT_REPLACEMENT_SPAWN" '.tab_id')"
read -r EXIT_RECOVERED <&"${EXIT_RECOVERY_WAIT[0]}"
wait "$EXIT_RECOVERY_WAIT_PROCESS"
[ "$(json_field "$EXIT_RECOVERED" '.owner_tab_id')" = "$EXIT_REPLACEMENT" ] || fail "exited-shell replacement observation"
EXIT_TEAMS="$(dc --json team list)"
EXIT_REPLACEMENT_VISIBLE="$(dc --json tab list --all-workspaces)"
node "$JSON_STATE" team-count "$EXIT_TEAM_ID" 1 <<<"$EXIT_TEAMS" || fail "exited-shell Team cardinality"
node "$JSON_STATE" live-tab "$EXIT_REPLACEMENT" <<<"$EXIT_REPLACEMENT_VISIBLE" || fail "exited-shell replacement liveness"
pass "process-exited Orchestrator is truthfully absent and replaced by one live Team owner"
dc --as-tab "$EXIT_REPLACEMENT" team close "$EXIT_TEAM_ID" --json >/dev/null
EXIT_TAB=""
EXIT_TEAM_ID=""
EXIT_REPLACEMENT=""

RELAUNCH_TITLE="LR-relaunch-$RUN_ID"
RELAUNCH_SESSION="$(node -e 'process.stdout.write(require("node:crypto").randomUUID())')"
RELAUNCH_STUB="$ROOT/evals/lifecycle-recovery/stub-agent.mjs"
RELAUNCH_PROBE="LR-probe-$RUN_ID"
DEV_DATA="$(dev_data_dir)"
DEV_DATA_NATIVE="$DEV_DATA"
if is_windows; then DEV_DATA_NATIVE="$(cygpath -w "$DEV_DATA")"; fi
JOURNAL="$DEV_DATA_NATIVE/sessions/tab-lifecycle.jsonl"
DEV_LOG="$DEV_DATA/logs/codeterm.log"
LOG_OFFSET=0
[ ! -f "$DEV_LOG" ] || LOG_OFFSET="$(( $(wc -c <"$DEV_LOG") ))"

# Each waiter's stdout is a descriptor this shell owns, so a waiter that
# resolves before its action returns leaves its line readable after it exits.
exec {RELAUNCH_EXIT_FD}< <(dev_run node "$ROOT/evals/lifecycle-recovery/wait-state.mjs" absent-shell "$RELAUNCH_TITLE")
RELAUNCH_EXIT_WAIT_PROCESS=$!
ready=""
read -r ready <&"$RELAUNCH_EXIT_FD" || true
[ "$ready" = READY ] || fail "in-tab relaunch exit observer readiness"
RELAUNCH_SPAWN="$(dc --json agent spawn claude --workspace "$WORKSPACE" --title "$RELAUNCH_TITLE" --non-user-facing --args --lifecycle-relaunch-fixture --stub-session "$RELAUNCH_SESSION")"
RELAUNCH_TAB="$(json_field "$RELAUNCH_SPAWN" '.tab_id')"
RELAUNCH_EXITED=""
read -r RELAUNCH_EXITED <&"$RELAUNCH_EXIT_FD" || true
wait "$RELAUNCH_EXIT_WAIT_PROCESS" || fail "in-tab relaunch exit observer"
exec {RELAUNCH_EXIT_FD}<&-
[ "$(json_field "$RELAUNCH_EXITED" '.tab_id')" = "$RELAUNCH_TAB" ] || fail "in-tab relaunch exit observation targeted spawned tab"
set +e
RELAUNCH_REFUSAL="$(dc --json agent restart --tab "$RELAUNCH_TAB" 2>&1)"
RELAUNCH_REFUSAL_RC=$?
set -e
[ "$RELAUNCH_REFUSAL_RC" -ne 0 ] || fail "absent process was advertised as restartable: $RELAUNCH_REFUSAL"
SHELL_RESTART="$(dc --json tab restart --tab "$RELAUNCH_TAB" 2>&1)" \
  || fail "tab restart refused the exited provider's plain shell: $SHELL_RESTART"

exec {RELAUNCH_READY_FD}< <(dev_run node "$ROOT/evals/lifecycle-recovery/wait-state.mjs" in-tab-ready "$RELAUNCH_TAB" "$RELAUNCH_SESSION" "$JOURNAL")
RELAUNCH_READY_WAIT_PROCESS=$!
ready=""
read -r ready <&"$RELAUNCH_READY_FD" || true
[ "$ready" = READY ] || fail "in-tab relaunch readiness observer readiness"
dc tab exec --tab "$RELAUNCH_TAB" "node '$RELAUNCH_STUB' --stub-provider claude --stub-session $RELAUNCH_SESSION --stub-report-bound" >/dev/null \
  || fail "tab exec relaunch in the same tab"
RELAUNCH_READY=""
read -r RELAUNCH_READY <&"$RELAUNCH_READY_FD" || true
wait "$RELAUNCH_READY_WAIT_PROCESS" || fail "in-tab relaunch readiness observer"
exec {RELAUNCH_READY_FD}<&-
[ "$(json_field "$RELAUNCH_READY" '.tab_id')" = "$RELAUNCH_TAB" ] \
  || fail "in-tab relaunch observed live, agent_ready, same session and launch_ready"
RELAUNCH_VISIBLE="$(dc --json tab list --all-workspaces)"
node "$JSON_STATE" live-tab "$RELAUNCH_TAB" <<<"$RELAUNCH_VISIBLE" || fail "in-tab relaunch is CLI-visible as live"

RELAUNCH_RESTART="$(dc --json agent restart --tab "$RELAUNCH_TAB" 2>&1)" \
  || fail "agent restart refused the observed in-tab relaunch: $RELAUNCH_RESTART"
[ "$(json_field "$RELAUNCH_RESTART" '.agent_ready // .data.agent_ready')" = true ] \
  || fail "agent restart of the in-tab relaunch reached agent_ready: $RELAUNCH_RESTART"

exec {RELAUNCH_SEND_FD}< <(dev_run node "$ROOT/evals/lifecycle-recovery/wait-state.mjs" received "$RELAUNCH_TAB" "$RELAUNCH_PROBE")
RELAUNCH_SEND_WAIT_PROCESS=$!
ready=""
read -r ready <&"$RELAUNCH_SEND_FD" || true
[ "$ready" = READY ] || fail "in-tab relaunch delivery observer readiness"
dc send "$RELAUNCH_PROBE" --tab "$RELAUNCH_TAB" >/dev/null || fail "send to the restarted in-tab relaunch"
RELAUNCH_RECEIVED=""
read -r RELAUNCH_RECEIVED <&"$RELAUNCH_SEND_FD" || true
wait "$RELAUNCH_SEND_WAIT_PROCESS" || fail "in-tab relaunch delivery observer"
exec {RELAUNCH_SEND_FD}<&-
[ "$(json_field "$RELAUNCH_RECEIVED" '.tab_id')" = "$RELAUNCH_TAB" ] || fail "stub received the routed send"
if [ -f "$DEV_LOG" ] && tail -c +"$((LOG_OFFSET + 1))" "$DEV_LOG" | grep -F "$RELAUNCH_TAB" | grep -qF tab_busy_refused; then
  fail "dev log recorded tab_busy_refused for the in-tab relaunch"
fi
pass "in-tab provider relaunch after exit is observed ready, restartable, and receives sends"

ORCH_TITLE="LR-owner-$RUN_ID"
coproc OWNER_WAIT { dev_run node "$ROOT/evals/lifecycle-recovery/wait-state.mjs" live "$ORCH_TITLE"; }
OWNER_WAIT_PROCESS="$OWNER_WAIT_PID"
read -r ready <&"${OWNER_WAIT[0]}"
[ "$ready" = READY ] || fail "owner observer readiness"
OWNER_SPAWN="$(dc --json agent spawn claude --workspace "$WORKSPACE" --title "$ORCH_TITLE" --role Orchestrator --non-user-facing)"
OLD_ORCHESTRATOR="$(json_field "$OWNER_SPAWN" '.tab_id')"
read -r OWNER_OBSERVED <&"${OWNER_WAIT[0]}"
wait "$OWNER_WAIT_PROCESS"
[ "$(json_field "$OWNER_OBSERVED" '.tab_id')" = "$OLD_ORCHESTRATOR" ] || fail "owner live observation"

PLAN_CREATE="$(dc --as-tab "$OLD_ORCHESTRATOR" --json plan create --title "LR-plan-$RUN_ID" --project "$ROOT")"
PLAN_ID="$(json_field "$PLAN_CREATE" '.data.plan_id // .data.plan.id // .plan_id // .plan.id')"
TEAM_ID="$(json_field "$PLAN_CREATE" '.data.team_id // .data.plan.team_id // .team_id // .plan.team_id')"
[ -n "$PLAN_ID" ] && [ "$PLAN_ID" != null ] || fail "plan creation: $PLAN_CREATE"
[ -n "$TEAM_ID" ] && [ "$TEAM_ID" != null ] || fail "team binding: $PLAN_CREATE"

dc --as-tab "$OLD_ORCHESTRATOR" agent unlink --tab "$OLD_ORCHESTRATOR" >/dev/null
TEAMS_AFTER_UNLINK="$(dc --json team list)"
node "$JSON_STATE" team-count "$TEAM_ID" 1 <<<"$TEAMS_AFTER_UNLINK" \
  || fail "final Orchestrator unlink erased Team"
PLAN_AFTER_UNLINK="$(dc --json plan get "$PLAN_ID")"
node "$JSON_STATE" plan-state "$PLAN_ID" "$TEAM_ID" null <<<"$PLAN_AFTER_UNLINK" \
  || fail "final Orchestrator unlink broke Plan continuity"

coproc RECOVERY_WAIT { dev_run node "$ROOT/evals/lifecycle-recovery/wait-state.mjs" recovered "$TEAM_ID" "$PLAN_ID"; }
RECOVERY_WAIT_PROCESS="$RECOVERY_WAIT_PID"
read -r ready <&"${RECOVERY_WAIT[0]}"
[ "$ready" = READY ] || fail "recovery observer readiness"
REPLACEMENT_SPAWN="$(dc --json agent spawn claude --team "$TEAM_ID" --role Orchestrator)"
REPLACEMENT="$(json_field "$REPLACEMENT_SPAWN" '.tab_id')"
read -r RECOVERED <&"${RECOVERY_WAIT[0]}"
wait "$RECOVERY_WAIT_PROCESS"
[ "$(json_field "$RECOVERED" '.owner_tab_id')" = "$REPLACEMENT" ] || fail "replacement owner observation"

TEAMS_FINAL="$(dc --json team list)"
PLAN_FINAL="$(dc --json plan get "$PLAN_ID")"
REPLACEMENT_VISIBLE="$(dc --json tab list --all-workspaces)"
node "$JSON_STATE" team-count "$TEAM_ID" 1 <<<"$TEAMS_FINAL" || fail "Team cardinality"
node "$JSON_STATE" plan-state "$PLAN_ID" "$TEAM_ID" "$REPLACEMENT" 2 <<<"$PLAN_FINAL" || fail "Plan identity and owner history"
node "$JSON_STATE" live-tab "$REPLACEMENT" <<<"$REPLACEMENT_VISIBLE" || fail "replacement liveness"
pass "final Orchestrator replacement keeps one Team and the original Plan history"
