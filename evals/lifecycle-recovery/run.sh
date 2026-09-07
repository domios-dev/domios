#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
source "$ROOT/evals/_lib/lib.sh"

fail() { echo "FAIL $1" >&2; exit 1; }
pass() { echo "PASS $1"; }
json_field() { jq -r "$2" <<<"$1"; }

HEALTH="$(dev_get /api/health)"
EXPECTED_SHA="$(git -C "$ROOT" rev-parse HEAD)"
[ "$(json_field "$HEALTH" '.build_sha')" = "$EXPECTED_SHA" ] || fail "build SHA mismatch"

RUN_ID="${EXPECTED_SHA:0:8}-$$"
WORKSPACE=""
EXIT_TAB=""
EXIT_TEAM_ID=""
EXIT_REPLACEMENT=""
OLD_ORCHESTRATOR=""
REPLACEMENT=""
TEAM_ID=""
cleanup() {
  [ -z "$TEAM_ID" ] || [ -z "$REPLACEMENT" ] || dc --as-tab "$REPLACEMENT" team close "$TEAM_ID" --json >/dev/null 2>&1 || true
  [ -z "$EXIT_TEAM_ID" ] || [ -z "$EXIT_REPLACEMENT" ] || dc --as-tab "$EXIT_REPLACEMENT" team close "$EXIT_TEAM_ID" --json >/dev/null 2>&1 || true
  for tab in "$EXIT_TAB" "$OLD_ORCHESTRATOR"; do
    [ -z "$tab" ] || dc tab close "$tab" >/dev/null 2>&1 || true
  done
  [ -z "$WORKSPACE" ] || dc workspace close "$WORKSPACE" >/dev/null 2>&1 || true
}
trap cleanup EXIT

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
EXIT_VISIBLE="$(dc --json tab inspect --tab "$EXIT_TAB")"
jq -e --arg tab "$EXIT_TAB" --arg team "$EXIT_TEAM_ID" '.. | objects | select(.id? == $tab and .agent_liveness? == "absent" and .agent_type? == null and .boot_stage? == "shell_ready" and .role_profile? == "orchestrator" and .team_id? == $team)' <<<"$EXIT_VISIBLE" >/dev/null \
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
EXIT_REPLACEMENT_VISIBLE="$(dc --json tab inspect --tab "$EXIT_REPLACEMENT")"
jq -e --arg team "$EXIT_TEAM_ID" '[.teams[] | select(.id == $team)] | length == 1' <<<"$EXIT_TEAMS" >/dev/null || fail "exited-shell Team cardinality"
jq -e --arg owner "$EXIT_REPLACEMENT" '.. | objects | select(.id? == $owner and .agent_liveness? == "live")' <<<"$EXIT_REPLACEMENT_VISIBLE" >/dev/null || fail "exited-shell replacement liveness"
pass "process-exited Orchestrator is truthfully absent and replaced by one live Team owner"
dc --as-tab "$EXIT_REPLACEMENT" team close "$EXIT_TEAM_ID" --json >/dev/null
EXIT_TAB=""
EXIT_TEAM_ID=""
EXIT_REPLACEMENT=""

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
PLAN_ID="$(json_field "$PLAN_CREATE" '.plan_id // .plan.id')"
TEAM_ID="$(json_field "$PLAN_CREATE" '.team_id // .plan.team_id')"
[ -n "$PLAN_ID" ] && [ "$PLAN_ID" != null ] || fail "plan creation"
[ -n "$TEAM_ID" ] && [ "$TEAM_ID" != null ] || fail "team binding"

dc --as-tab "$OLD_ORCHESTRATOR" agent unlink --tab "$OLD_ORCHESTRATOR" >/dev/null
TEAMS_AFTER_UNLINK="$(dc --json team list)"
jq -e --arg team "$TEAM_ID" '[.teams[] | select(.id == $team)] | length == 1' <<<"$TEAMS_AFTER_UNLINK" >/dev/null \
  || fail "final Orchestrator unlink erased Team"
PLAN_AFTER_UNLINK="$(dc --json plan get "$PLAN_ID")"
jq -e --arg plan "$PLAN_ID" --arg team "$TEAM_ID" '(.plan // .) | .id == $plan and .team_id == $team and .status == "active" and .owner.tab_id == null' <<<"$PLAN_AFTER_UNLINK" >/dev/null \
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
REPLACEMENT_VISIBLE="$(dc --json tab inspect --tab "$REPLACEMENT")"
jq -e --arg team "$TEAM_ID" '[.teams[] | select(.id == $team)] | length == 1' <<<"$TEAMS_FINAL" >/dev/null || fail "Team cardinality"
jq -e --arg plan "$PLAN_ID" --arg team "$TEAM_ID" --arg owner "$REPLACEMENT" '(.plan // .) | .id == $plan and .team_id == $team and .status == "active" and .owner.tab_id == $owner and (.owner_history | length >= 2)' <<<"$PLAN_FINAL" >/dev/null || fail "Plan identity and owner history"
jq -e --arg owner "$REPLACEMENT" '.. | objects | select(.id? == $owner and .agent_liveness? == "live")' <<<"$REPLACEMENT_VISIBLE" >/dev/null || fail "replacement liveness"
pass "final Orchestrator replacement keeps one Team and the original Plan history"
