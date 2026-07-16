#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$ROOT/evals/_lib/lib.sh"
CREATED=()
TEAM_ID=""
WORKER=""
first_assistant() { python -c 'import json,sys;d=json.load(sys.stdin);ms=d.get("messages",d if isinstance(d,list) else []);print(next((str(m.get("content",m.get("text",""))) for m in ms if str(m.get("role",m.get("author",""))).lower()=="assistant"),""))'; }
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
SYSTEM=$(python -c 'import json,sys;d=json.load(sys.stdin);print(next((x["id"] for x in d if x.get("system_prompt_delivery")!="external"),""))' <<<"$PROVIDERS")
FALLBACK=$(python -c 'import json,sys;d=json.load(sys.stdin);print(next((x["id"] for x in d if x.get("system_prompt_delivery")=="external"),""))' <<<"$PROVIDERS")
[ -n "$SYSTEM" ] || { echo "SKIP: no system-prompt provider"; exit 0; }
PAIR=$(dc orchestration new --orchestrator "$SYSTEM" --planner "$SYSTEM" --workspace "$WS" --json)
ORCH=$(python -c 'import json,sys;print(json.load(sys.stdin).get("orchestrator_pane_id", ""))' <<<"$PAIR")
PLANNER=$(python -c 'import json,sys;print(json.load(sys.stdin).get("planner_pane_id", ""))' <<<"$PAIR")
CREATED+=("$ORCH" "$PLANNER")
TEAM_ID=$(dc team list --json | python -c 'import json,sys;before={x.get("id") for x in json.loads(sys.argv[1])}; now=json.load(sys.stdin); fresh=[x for x in now if x.get("id") not in before]; assert len(fresh)==1; print(fresh[0]["id"])' "$TEAMS_BEFORE")
STAFF=$(python -c 'import json,sys;print(json.dumps({"provider":sys.argv[1],"role":"planner","title":"manager-bootstrap-staffed","task":None,"model":None}))' "$SYSTEM")
STAFFED=$(dev_post_as_pane "$ORCH" "/api/teams/$TEAM_ID/members" "$STAFF" | python -c 'import json,sys;d=json.load(sys.stdin);print(d.get("pane_id", d.get("member",{}).get("pane_id","")))')
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
  FALLBACK_PANE=$(dc agent spawn "$FALLBACK" --role Orchestrator --workspace "$WS" --cwd "$ROOT" --title "manager-bootstrap-fallback" --json | python -c 'import json,sys;print(json.load(sys.stdin).get("pane_id", ""))')
  CREATED+=("$FALLBACK_PANE")
  dc pane send --pane "$FALLBACK_PANE" "Who are you?" >/dev/null
  [[ "$(await_first "$FALLBACK_PANE")" == *Orchestrator* ]]
else
  echo "SKIP: no fallback-capable provider"
fi
dc workspace close "$WS" >/dev/null
