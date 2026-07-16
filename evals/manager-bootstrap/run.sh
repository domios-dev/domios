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
PAIR=$(dc orchestration new --orchestrator claude:opus --planner claude:fable --workspace "$WS" --json)
ORCH=$(python -c 'import json,sys;print(json.load(sys.stdin).get("orchestrator_pane_id", ""))' <<<"$PAIR")
PLANNER=$(python -c 'import json,sys;print(json.load(sys.stdin).get("planner_pane_id", ""))' <<<"$PAIR")
CREATED+=("$ORCH" "$PLANNER")
for pane in "$ORCH" "$PLANNER"; do
  dc pane send --pane "$pane" "Who are you? What team and project are you in?" >/dev/null
  dc pane chat --pane "$pane" --json | python -c 'import json,sys; assert "Orchestrator" in str(json.load(sys.stdin)) or "Planner" in str(json.load(sys.stdin))'
done
WORKER=$(dc agent spawn claude --role Worker --parent "$ORCH" --workspace "$WS" --cwd "$ROOT" --title "manager-bootstrap-worker" --json | python -c 'import json,sys;print(json.load(sys.stdin).get("pane_id", ""))')
CREATED+=("$WORKER")
dc pane send --pane "$WORKER" "Who are you?" >/dev/null
dc pane chat --pane "$WORKER" --json | python -c 'import json,sys; assert "Worker" in str(json.load(sys.stdin))'
if dc provider list --json | grep -q '"system_prompt_delivery":"external"'; then
  echo "fallback provider available; fixture is exercised by the same pair assertions"
fi
dc workspace close "$WS" >/dev/null
