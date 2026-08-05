#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$ROOT/evals/_lib/lib.sh"
source "$ROOT/evals/_lib/role_preset.sh"
source "$(dirname "${BASH_SOURCE[0]}")/helpers.sh"
CREATED=()
TEAM_ID=""
FALLBACK_TEAM_ID=""
WORKER=""
cleanup() {
  [ -z "$FALLBACK_TEAM_ID" ] || dc team close "$FALLBACK_TEAM_ID" --json >/dev/null 2>&1 || true
  [ -z "$TEAM_ID" ] || dc team close "$TEAM_ID" --json >/dev/null 2>&1 || true
  for tab in "${CREATED[@]:-}"; do dc tab close "$tab" >/dev/null 2>&1 || true; done
  ((${#CREATED[@]})) || return 0
  MUX_STATE=$(dev_get /api/mux/state)
  mux_absent "${CREATED[@]}" <<<"$MUX_STATE"
}
trap cleanup EXIT
dev_get /api/health >/dev/null || { echo "dev 7686 unavailable" >&2; exit 7; }
# Every manager kind bootstraps from the one shared resolver, and an unmatched
# kind lands on the declared fallback instead of an empty identity.
assert_manager_kinds_resolve
assert_planner_team_authority
assert_declared_fallback
WS=$(new_workspace "manager-bootstrap-eval" "$ROOT")
TEAMS_BEFORE=$(capture_json dc team list --json)
PROVIDERS=$(capture_json dc agent providers --json)
SYSTEM=$(provider_id system <<<"$PROVIDERS")
FALLBACK=$(provider_id external <<<"$PROVIDERS")
[ -n "$SYSTEM" ] || { echo "SKIP: no system-prompt provider"; exit 0; }
PAIR=$(capture_json dc team pair --orchestrator "$SYSTEM" --planner "$SYSTEM" --workspace "$WS" --json)
ORCH=$(pair_tab_id orchestrator <<<"$PAIR")
PLANNER=$(pair_tab_id planner <<<"$PAIR")
[ -n "$ORCH" ] && [ -n "$PLANNER" ] || { echo "pair returned empty tab id" >&2; exit 1; }
CREATED+=("$ORCH" "$PLANNER")
TEAMS_AFTER=$(capture_json dc team list --json)
TEAM_ID=$(new_team_id "$TEAMS_BEFORE" <<<"$TEAMS_AFTER")
[ -n "$TEAM_ID" ] || { echo "pair returned no Team" >&2; exit 1; }
STAFF=$(staff_request "$SYSTEM")
STAFFED_RESPONSE=$(dev_post_as_tab "$ORCH" "/api/teams/$TEAM_ID/members" "$STAFF")
STAFFED=$(staffed_tab_id <<<"$STAFFED_RESPONSE")
[ -n "$STAFFED" ] || { echo "staffed manager returned empty tab id" >&2; exit 1; }
CREATED+=("$STAFFED")
dc send --tab "$STAFFED" "Who are you? What team and project are you in?" >/dev/null
STAFF_REPLY="$(await_first "$STAFFED")"; [[ "$STAFF_REPLY" == *Planner* && "$STAFF_REPLY" == *manager-bootstrap-eval* && "$STAFF_REPLY" == *"$TEAM_ID"* ]]
for tab in "$ORCH" "$PLANNER"; do
  dc send --tab "$tab" "Who are you? What team and project are you in?" >/dev/null
  REPLY="$(await_first "$tab")"; [[ "$REPLY" == *manager-bootstrap-eval* && "$REPLY" == *"not yet established"* ]]; case "$tab" in "$ORCH") [[ "$REPLY" == *Orchestrator* ]];; *) [[ "$REPLY" == *Planner* ]];; esac
done
WORKER_RESPONSE=$(capture_json dc agent spawn "$SYSTEM" --role Worker --parent "$ORCH" --workspace "$WS" --cwd "$ROOT" --title "manager-bootstrap-worker" --json)
WORKER=$(spawned_tab_id <<<"$WORKER_RESPONSE")
CREATED+=("$WORKER")
dc send --tab "$WORKER" "Who are you?" >/dev/null
[[ "$(await_first "$WORKER")" == *Worker* ]]
if [ -n "$FALLBACK" ]; then
  FALLBACK_TEAMS_BEFORE=$(capture_json dc team list --json)
  FALLBACK_PAIR=$(capture_json dc team pair --orchestrator "$FALLBACK" --planner "$FALLBACK" --workspace "$WS" --json || true)
  FALLBACK_ORCH=$(pair_tab_id orchestrator <<<"$FALLBACK_PAIR")
  FALLBACK_PLANNER=$(pair_tab_id planner <<<"$FALLBACK_PAIR")
  if [ -n "$FALLBACK_ORCH" ] && [ -n "$FALLBACK_PLANNER" ]; then
    FALLBACK_TEAMS_AFTER=$(capture_json dc team list --json)
    FALLBACK_TEAM_ID=$(new_team_id "$FALLBACK_TEAMS_BEFORE" <<<"$FALLBACK_TEAMS_AFTER")
    [ -n "$FALLBACK_TEAM_ID" ] || exit 1
    CREATED+=("$FALLBACK_ORCH" "$FALLBACK_PLANNER")
    for tab in "$FALLBACK_ORCH" "$FALLBACK_PLANNER"; do dc send --tab "$tab" "Who are you? What project are you in?" >/dev/null; REPLY="$(await_first "$tab")"; [[ "$REPLY" == *manager-bootstrap-eval* && "$REPLY" == *"not yet established"* && "$REPLY" != *"User work"* ]]; case "$tab" in "$FALLBACK_ORCH") [[ "$REPLY" == *Orchestrator* ]];; *) [[ "$REPLY" == *Planner* ]];; esac; done
  else echo "SKIP: fallback descriptor cannot create orchestration pair"; fi
else
  echo "SKIP: no fallback-capable provider"
fi
CLOSED_TEAM="$TEAM_ID"
dc team close "$CLOSED_TEAM" --json >/dev/null
if [ -n "$FALLBACK_TEAM_ID" ]; then dc team close "$FALLBACK_TEAM_ID" --json >/dev/null; CLOSED_TEAMS="$CLOSED_TEAM $FALLBACK_TEAM_ID"; FALLBACK_TEAM_ID=""; else CLOSED_TEAMS="$CLOSED_TEAM"; fi
TEAM_ID=""
for team in $CLOSED_TEAMS; do ! dev_get /api/mux/state | grep -q "$team"; done
dc workspace close "$WS" >/dev/null
