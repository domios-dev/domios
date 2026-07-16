#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$ROOT/evals/_lib/lib.sh"
CREATED=()
WORKER=""
cleanup() {
  for pane in "${CREATED[@]:-}"; do dc pane close "$pane" >/dev/null 2>&1 || true; done
  dev_get /api/mux/state | python -c 'import json,sys;d=json.load(sys.stdin); ids=set(sys.argv[1:]); assert not any(str(p.get("id")) in ids for p in d.get("panes",[]))' "${CREATED[@]:-}"
}
trap cleanup EXIT
dev_get /api/health >/dev/null || { echo "dev 7686 unavailable" >&2; exit 7; }
WS=$(new_workspace "manager-bootstrap-eval" "$ROOT")
PROVIDERS=$(dc agent providers --json)
SYSTEM=$(python -c 'import json,sys;d=json.load(sys.stdin);print(next((x["id"] for x in d if x.get("system_prompt_delivery")!="external"),""))' <<<"$PROVIDERS")
FALLBACK=$(python -c 'import json,sys;d=json.load(sys.stdin);print(next((x["id"] for x in d if x.get("system_prompt_delivery")=="external"),""))' <<<"$PROVIDERS")
[ -n "$SYSTEM" ] || { echo "SKIP: no system-prompt provider"; exit 0; }
PAIR=$(dc orchestration new --orchestrator "$SYSTEM" --planner "$SYSTEM" --workspace "$WS" --json)
ORCH=$(python -c 'import json,sys;print(json.load(sys.stdin).get("orchestrator_pane_id", ""))' <<<"$PAIR")
PLANNER=$(python -c 'import json,sys;print(json.load(sys.stdin).get("planner_pane_id", ""))' <<<"$PAIR")
CREATED+=("$ORCH" "$PLANNER")
for pane in "$ORCH" "$PLANNER"; do
  dc pane send --pane "$pane" "Who are you? What team and project are you in?" >/dev/null
  dc pane chat --pane "$pane" --json | python -c 'import json,sys;d=json.load(sys.stdin);s=str(d);assert ("Orchestrator" in s or "Planner" in s) and "manager-bootstrap-eval" in s and "not yet established" in s'
done
WORKER=$(dc agent spawn "$SYSTEM" --role Worker --parent "$ORCH" --workspace "$WS" --cwd "$ROOT" --title "manager-bootstrap-worker" --json | python -c 'import json,sys;print(json.load(sys.stdin).get("pane_id", ""))')
CREATED+=("$WORKER")
dc pane send --pane "$WORKER" "Who are you?" >/dev/null
dc pane chat --pane "$WORKER" --json | python -c 'import json,sys; assert "Worker" in str(json.load(sys.stdin))'
if [ -n "$FALLBACK" ]; then
  FALLBACK_PANE=$(dc agent spawn "$FALLBACK" --role Orchestrator --workspace "$WS" --cwd "$ROOT" --title "manager-bootstrap-fallback" --json | python -c 'import json,sys;print(json.load(sys.stdin).get("pane_id", ""))')
  CREATED+=("$FALLBACK_PANE")
  dc pane send --pane "$FALLBACK_PANE" "Who are you?" >/dev/null
  dc pane chat --pane "$FALLBACK_PANE" --json | python -c 'import json,sys;assert "Orchestrator" in str(json.load(sys.stdin))'
else
  echo "SKIP: no fallback-capable provider"
fi
dc workspace close "$WS" >/dev/null
