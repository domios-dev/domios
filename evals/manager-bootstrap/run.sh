#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
source "$ROOT/evals/_lib/lib.sh"
CREATED=()
cleanup() {
  for pane in "${CREATED[@]:-}"; do dc pane close "$pane" >/dev/null 2>&1 || true; done
  dev_get /api/mux/state >/dev/null
}
trap cleanup EXIT
dev_get /api/health >/dev/null
WS=$(new_workspace "manager-bootstrap-eval" "$ROOT")
PAIR=$(dc orchestration new --orchestrator claude:opus --planner claude:fable --workspace "$WS" --json)
ORCH=$(python -c 'import json,sys;print(json.load(sys.stdin).get("orchestrator_pane_id", ""))' <<<"$PAIR")
PLANNER=$(python -c 'import json,sys;print(json.load(sys.stdin).get("planner_pane_id", ""))' <<<"$PAIR")
CREATED+=("$ORCH" "$PLANNER")
for pane in "$ORCH" "$PLANNER"; do
  dc pane send --pane "$pane" "Who are you? What team and project are you in?" >/dev/null
  dc pane chat --pane "$pane" --json | python -c 'import json,sys; assert "Orchestrator" in str(json.load(sys.stdin)) or "Planner" in str(json.load(sys.stdin))'
done
dc workspace close "$WS" >/dev/null
