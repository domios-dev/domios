#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$ROOT/evals/_lib/lib.sh"
source "$(dirname "${BASH_SOURCE[0]}")/helpers.sh"
CREATED=()
TEAM_ID=""
WORKER=""
await_first() { local pane="$1" end=$((SECONDS+45)) out; while [ "$SECONDS" -lt "$end" ]; do out="$(dc pane chat --pane "$pane" --json | first_assistant)"; [ -n "$out" ] && { printf '%s' "$out"; return; }; sleep 1; done; echo "timeout first assistant $pane" >&2; return 1; }
cleanup() {
  [ -z "$TEAM_ID" ] || dc team close "$TEAM_ID" --json >/dev/null 2>&1 || true
  for pane in "${CREATED[@]:-}"; do dc pane close "$pane" >/dev/null 2>&1 || true; done
  dev_get /api/mux/state | python -c 'import json,sys;d=json.load(sys.stdin); ids=set(sys.argv[1:]); assert not any(str(p.get("id")) in ids for p in d.get("panes",[]))' "${CREATED[@]:-}"
}
trap cleanup EXIT
dev_get /api/health >/dev/null || { echo "dev 7686 unavailable" >&2; exit 7; }
WS=$(new_workspace "manager-bootstrap-eval" "$ROOT")
TEAMS_BEFORE=$(dc team list --json)
PROVIDERS=$(dc agent providers --json)
SYSTEM=$(provider_id system <<<"$PROVIDERS")
FALLBACK=$(provider_id external <<<"$PROVIDERS")
[ -n "$SYSTEM" ] || { echo "SKIP: no system-prompt provider"; exit 0; }
PAIR=$(dc orchestration new --orchestrator "$SYSTEM" --planner "$SYSTEM" --workspace "$WS" --json)
ORCH=$(python -c 'import json,sys;print(json.load(sys.stdin).get("orchestrator_pane_id", ""))' <<<"$PAIR")
PLANNER=$(python -c 'import json,sys;print(json.load(sys.stdin).get("planner_pane_id", ""))' <<<"$PAIR")
[ -n "$ORCH" ] && [ -n "$PLANNER" ] || { echo "pair returned empty pane id" >&2; exit 1; }
CREATED+=("$ORCH" "$PLANNER")
TEAM_ID=$(dc team list --json | python -c 'import json,sys;before={x.get("id") for x in json.loads(sys.argv[1])}; now=json.load(sys.stdin); fresh=[x for x in now if x.get("id") not in before]; assert len(fresh)==1; print(fresh[0]["id"])' "$TEAMS_BEFORE")
[ -n "$TEAM_ID" ] || { echo "pair returned no Team" >&2; exit 1; }
STAFF=$(staff_request "$SYSTEM")
STAFFED=$(dev_post_as_pane "$ORCH" "/api/teams/$TEAM_ID/members" "$STAFF" | python -c 'import json,sys;d=json.load(sys.stdin);print(d.get("pane_id", d.get("member",{}).get("pane_id","")))')
[ -n "$STAFFED" ] || { echo "staffed manager returned empty pane id" >&2; exit 1; }
CREATED+=("$STAFFED")
dc pane send --pane "$STAFFED" "Who are you? What team and project are you in?" >/dev/null
STAFF_REPLY="$(await_first "$STAFFED")"; [[ "$STAFF_REPLY" == *Planner* && "$STAFF_REPLY" == *manager-bootstrap-eval* && "$STAFF_REPLY" == *"$TEAM_ID"* ]]
for pane in "$ORCH" "$PLANNER"; do
  dc pane send --pane "$pane" "Who are you? What team and project are you in?" >/dev/null
  REPLY="$(await_first "$pane")"; [[ "$REPLY" == *manager-bootstrap-eval* && "$REPLY" == *"not yet established"* ]]; case "$pane" in "$ORCH") [[ "$REPLY" == *Orchestrator* ]];; *) [[ "$REPLY" == *Planner* ]];; esac
done
WORKER=$(dc agent spawn "$SYSTEM" --role Worker --parent "$ORCH" --workspace "$WS" --cwd "$ROOT" --title "manager-bootstrap-worker" --json | python -c 'import json,sys;print(json.load(sys.stdin).get("pane_id", ""))')
CREATED+=("$WORKER")
dc pane send --pane "$WORKER" "Who are you?" >/dev/null
[[ "$(await_first "$WORKER")" == *Worker* ]]
if [ -n "$FALLBACK" ]; then
  FALLBACK_PAIR=$(dc orchestration new --orchestrator "$FALLBACK" --planner "$FALLBACK" --workspace "$WS" --json || true)
  FALLBACK_ORCH=$(python -c 'import json,sys;print(json.load(sys.stdin).get("orchestrator_pane_id", ""))' <<<"$FALLBACK_PAIR")
  FALLBACK_PLANNER=$(python -c 'import json,sys;print(json.load(sys.stdin).get("planner_pane_id", ""))' <<<"$FALLBACK_PAIR")
  if [ -n "$FALLBACK_ORCH" ] && [ -n "$FALLBACK_PLANNER" ]; then
    CREATED+=("$FALLBACK_ORCH" "$FALLBACK_PLANNER")
    for pane in "$FALLBACK_ORCH" "$FALLBACK_PLANNER"; do dc pane send --pane "$pane" "Who are you? What project are you in?" >/dev/null; REPLY="$(await_first "$pane")"; [[ "$REPLY" == *manager-bootstrap-eval* && "$REPLY" == *"not yet established"* && "$REPLY" != *"User work"* ]]; done
  else echo "SKIP: fallback descriptor cannot create orchestration pair"; fi
else
  echo "SKIP: no fallback-capable provider"
fi
CLOSED_TEAM="$TEAM_ID"
dc team close "$CLOSED_TEAM" --json >/dev/null
TEAM_ID=""
! dev_get /api/mux/state | grep -q "$CLOSED_TEAM"
dc workspace close "$WS" >/dev/null
