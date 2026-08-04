#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
source "$ROOT/evals/_lib/lib.sh"

TIMEOUT="${CODETERM_TRUST_TIMEOUT:-90}"
CREATED_TABS=()
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

tab_value() {
  dev_get /api/mux/state | python -c '
import json, sys
tab = json.load(sys.stdin).get("tabs", {}).get(sys.argv[1], {})
value = tab.get(sys.argv[2])
if value is None:
    raise SystemExit(1)
print(json.dumps(value) if isinstance(value, (dict, list)) else value)
' "$1" "$2"
}

tab_by_title() {
  dev_get /api/tabs | python -c '
import json, sys
for tab in json.load(sys.stdin):
    if tab.get("title") == sys.argv[1]:
        print(tab["id"])
        break
' "$1"
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
      -H "X-CodeTerm-Tab-Id: $MANAGER_ID" -H "Content-Type: application/json" \
      --data '{}' "http://127.0.0.1:7686/api/teams/$TEAM_ID/close" >/dev/null
  fi
  for tab in "${CREATED_TABS[@]:-}"; do
    [ -z "$tab" ] || curl -s --max-time 15 -X POST -H "$CODETERM_DEV_AUTH_HEADER" \
      -H "Content-Type: application/json" --data '{}' \
      "http://127.0.0.1:7686/api/tabs/$tab/close" >/dev/null || true
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
MANAGER_ID="$(printf '%s' "$manager_json" | json_value tab_id)"
CREATED_TABS+=("$MANAGER_ID")
dc agent set-orchestrator --tab "$MANAGER_ID" >/dev/null

plan_body="$(python -c 'import json,sys; print(json.dumps({
    "title": sys.argv[1],
    "project_path": sys.argv[2],
    "owner": {"tab_id": sys.argv[3]},
}))' "CODEX-TRUST-$$" "$ROOT" "$MANAGER_ID")"
plan_json="$(curl -s --max-time 30 -X POST -H "$CODETERM_DEV_AUTH_HEADER" \
  -H "Content-Type: application/json" --data "$plan_body" \
  http://127.0.0.1:7686/api/plans)"
PLAN_ID="$(printf '%s' "$plan_json" | json_value plan_id)"
TEAM_ID="$(printf '%s' "$plan_json" | json_value team_id)"

dispatch_worker() {
  local suffix="$1" body dispatch tab cwd
  body="$(python -c 'import json,sys; print(json.dumps({
      "provider": "codex", "role": "worker", "title": sys.argv[1]
  }))' "ct-trust-worker-$suffix")"
  dispatch="$(curl -s --max-time 90 -X POST -H "$CODETERM_DEV_AUTH_HEADER" \
    -H "X-CodeTerm-Tab-Id: $MANAGER_ID" -H "Content-Type: application/json" \
    --data "$body" "http://127.0.0.1:7686/api/teams/$TEAM_ID/members")"
  tab="$(printf '%s' "$dispatch" | json_value tab_id)"
  CREATED_TABS+=("$tab")
  wait_for "worker $suffix agent_ready" \
    "[ \"\$(tab_value '$tab' boot_stage 2>/dev/null || true)\" = agent_ready ]"
  cwd="$(tab_value "$tab" cwd)"
  [ "$cwd" != "$ROOT" ] || { echo "FAIL: worker $suffix stayed in shared checkout" >&2; return 1; }
  [ -d "$cwd/.git" ] || [ -f "$cwd/.git" ] || { echo "FAIL: worker $suffix cwd is not a worktree" >&2; return 1; }
  LANE_CWDS+=("$cwd")
  LAST_TAB="$tab"
}

LAST_TAB=""
dispatch_worker a
first="$LAST_TAB"
dispatch_worker b
second="$LAST_TAB"
[ "${LANE_CWDS[0]}" != "${LANE_CWDS[1]}" ] || { echo "FAIL: workers reused one lane" >&2; exit 1; }

mkdir -p "$FOREIGN"
foreign_title="ct-trust-foreign-$$"
set +e
dc agent spawn codex --role Orchestrator --workspace "$workspace" \
  --cwd "$FOREIGN" --json --title "$foreign_title" >/dev/null 2>&1
set -e
wait_for "foreign tab creation" "[ -n \"\$(tab_by_title '$foreign_title')\" ]"
foreign_tab="$(tab_by_title "$foreign_title")"
CREATED_TABS+=("$foreign_tab")
wait_for "foreign question state" \
  "tab_value '$foreign_tab' agent_state 2>/dev/null | grep -q '\"state\": \"question\"'"
dc tab get-text --tab "$foreign_tab" | grep -qi "trust the contents of this directory" || {
  echo "FAIL: foreign trust prompt is not visible in terminal text" >&2
  exit 1
}
[ "$(tab_value "$foreign_tab" cwd)" = "$FOREIGN" ] || { echo "FAIL: foreign cwd changed" >&2; exit 1; }
[ "$(tab_value "$foreign_tab" boot_stage 2>/dev/null || true)" != agent_ready ] || {
  echo "FAIL: foreign tab bypassed the trust prompt" >&2
  exit 1
}

echo "PASS: provisioned workers $first/$second reached agent_ready in distinct lanes"
echo "PASS: foreign tab $foreign_tab retained the directory-trust prompt"
