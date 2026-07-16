#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
source "$ROOT/evals/_lib/lib.sh"

TIMEOUT="${CODETERM_TRUST_TIMEOUT:-90}"
CREATED_PANES=()
LANE_CWDS=()
PLAN_ID=""
MANAGER_ID=""
TEAM_ID=""
FOREIGN="$ROOT/../codeterm-lookalike-worktrees/trust-foreign-$$"

json_value() {
  python -c '
import json, sys
key = sys.argv[1]
data = json.load(sys.stdin)
def find(value):
    if isinstance(value, dict):
        if key in value and value[key] not in (None, ""):
            return value[key]
        for child in value.values():
            found = find(child)
            if found not in (None, ""):
                return found
    if isinstance(value, list):
        for child in value:
            found = find(child)
            if found not in (None, ""):
                return found
    return None
value = find(data)
if value is None:
    raise SystemExit(1)
print(value)
' "$1"
}

pane_value() {
  dev_get /api/mux/state | python -c '
import json, sys
pane = json.load(sys.stdin).get("panes", {}).get(sys.argv[1], {})
value = pane.get(sys.argv[2])
if value is None:
    raise SystemExit(1)
print(json.dumps(value) if isinstance(value, (dict, list)) else value)
' "$1" "$2"
}

wait_for() {
  local description="$1" predicate="$2" deadline=$((SECONDS + TIMEOUT))
  until eval "$predicate"; do
    if [ "$SECONDS" -ge "$deadline" ]; then
      echo "FAIL: timed out waiting for $description" >&2
      return 1
    fi
    sleep 1
  done
}

cleanup_lane() {
  local cwd="$1" branch
  [ -n "$cwd" ] && [ "$cwd" != "$ROOT" ] || return 0
  branch="$(git -C "$cwd" branch --show-current 2>/dev/null || true)"
  git -C "$ROOT" worktree remove --force "$cwd" >/dev/null 2>&1 || true
  [ -z "$branch" ] || git -C "$ROOT" branch -D "$branch" >/dev/null 2>&1 || true
}

cleanup() {
  set +e
  if [ -n "$TEAM_ID" ] && [ -n "$MANAGER_ID" ]; then
    curl -s --max-time 30 -X POST -H "$CODETERM_DEV_AUTH_HEADER" \
      -H "X-CodeTerm-Pane-Id: $MANAGER_ID" -H "Content-Type: application/json" \
      --data '{}' "http://127.0.0.1:7686/api/teams/$TEAM_ID/close" >/dev/null
  fi
  for pane in "${CREATED_PANES[@]:-}"; do
    [ -z "$pane" ] || dc tab close "$pane" >/dev/null 2>&1 || true
  done
  for cwd in "${LANE_CWDS[@]:-}"; do cleanup_lane "$cwd"; done
  [ -z "$PLAN_ID" ] || dc plan delete "$PLAN_ID" --yes >/dev/null 2>&1 || true
  rm -rf "$FOREIGN"
}
trap cleanup EXIT

health="$(dev_get /api/health)"
build="$(printf '%s' "$health" | json_value build)"
head="$(git rev-parse HEAD)"
case "$head" in "$build"*) ;; *) echo "FAIL: dev build $build is not HEAD $head" >&2; exit 1;; esac

workspace="$(dev_get /api/workspaces | python -c '
import json, os, sys
root = os.path.normcase(os.path.abspath(sys.argv[1]))
for item in json.load(sys.stdin):
    cwd = item.get("cwd")
    if cwd and os.path.normcase(os.path.abspath(cwd)) == root:
        print(item["id"])
        break
' "$ROOT")"
[ -n "$workspace" ] || { echo "FAIL: no dev workspace for $ROOT" >&2; exit 1; }

manager_json="$(dc tab new --workspace "$workspace" --cwd "$ROOT" --title ct-trust-manager --eager --json)"
MANAGER_ID="$(printf '%s' "$manager_json" | json_value pane_id)"
CREATED_PANES+=("$MANAGER_ID")
dc agent set-orchestrator --tab "$MANAGER_ID" >/dev/null

plan_json="$(dc plan create --title "CODEX-TRUST-$$" --project "$ROOT" --json)"
PLAN_ID="$(printf '%s' "$plan_json" | json_value plan_id)"

dispatch_worker() {
  local suffix="$1" task_json task_id dispatch pane cwd
  task_json="$(dc plan task add "$PLAN_ID" --name "trust worker $suffix" \
    --type test --model gpt-5.5 --executor-role test-infra-dev \
    --body "Verify provisioned Codex trust for worker $suffix." \
    --criterion "Worker reaches agent-ready without manual trust input." \
    --test-plan "Inspect the exact dev mux state for agent_ready." --json)"
  task_id="$(printf '%s' "$task_json" | json_value id)"
  dispatch="$(dc plan task dispatch "$PLAN_ID" "$task_id" --agent codex \
    --parent-tab "$MANAGER_ID" --workspace "$workspace" --cwd "$ROOT" --json)"
  pane="$(printf '%s' "$dispatch" | json_value pane_id)"
  CREATED_PANES+=("$pane")
  wait_for "worker $suffix agent_ready" \
    "[ \"\$(pane_value '$pane' boot_stage 2>/dev/null || true)\" = agent_ready ]"
  cwd="$(pane_value "$pane" cwd)"
  [ "$cwd" != "$ROOT" ] || { echo "FAIL: worker $suffix stayed in shared checkout" >&2; return 1; }
  [ -d "$cwd/.git" ] || [ -f "$cwd/.git" ] || { echo "FAIL: worker $suffix cwd is not a worktree" >&2; return 1; }
  LANE_CWDS+=("$cwd")
  LAST_PANE="$pane"
}

LAST_PANE=""
dispatch_worker a
first="$LAST_PANE"
dispatch_worker b
second="$LAST_PANE"
[ "${LANE_CWDS[0]}" != "${LANE_CWDS[1]}" ] || { echo "FAIL: workers reused one lane" >&2; exit 1; }
TEAM_ID="$(pane_value "$MANAGER_ID" team_id)"

mkdir -p "$FOREIGN"
foreign_json="$(dc agent spawn codex --role Orchestrator --workspace "$workspace" \
  --cwd "$FOREIGN" --title ct-trust-foreign --json)"
foreign_pane="$(printf '%s' "$foreign_json" | json_value pane_id)"
CREATED_PANES+=("$foreign_pane")
wait_for "foreign trust prompt" \
  "pane_value '$foreign_pane' pending_prompt 2>/dev/null | grep -qi 'trust the contents of this directory'"
[ "$(pane_value "$foreign_pane" cwd)" = "$FOREIGN" ] || { echo "FAIL: foreign cwd changed" >&2; exit 1; }
[ "$(pane_value "$foreign_pane" boot_stage 2>/dev/null || true)" != agent_ready ] || {
  echo "FAIL: foreign pane bypassed the trust prompt" >&2
  exit 1
}

echo "PASS: provisioned workers $first/$second reached agent_ready in distinct lanes"
echo "PASS: foreign pane $foreign_pane retained the directory-trust prompt"
