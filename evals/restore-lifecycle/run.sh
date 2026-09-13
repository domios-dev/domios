#!/usr/bin/env bash
# Prove every agent tab of a Team returns alive, bound to its own session, after
# full dev restarts, including one taken while an agent is mid-turn.
set -u
set -o pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
source "$REPO_ROOT/evals/_lib/lib.sh"

RL_CYCLES="${RL_CYCLES:-2}"
RL_MIDTURN_CYCLE="${RL_MIDTURN_CYCLE:-2}"
RL_BIND_DEADLINE="${RL_BIND_DEADLINE:-180}"
RL_TURN_DEADLINE="${RL_TURN_DEADLINE:-180}"
RL_REPORT_DEADLINE="${RL_REPORT_DEADLINE:-240}"
RL_CLAUDE_MODEL="${RL_CLAUDE_MODEL:-haiku}"
RL_CODEX_MODEL="${RL_CODEX_MODEL:-gpt-5.4-mini}"

RUN_ID="$(python3 -c 'import uuid; print(uuid.uuid4().hex[:10])')"
WORKSPACE_NAME="RESTORE-LIFECYCLE-$RUN_ID"
if is_windows; then WORKSPACE_CWD="C:/temp/$WORKSPACE_NAME"; else WORKSPACE_CWD="/tmp/$WORKSPACE_NAME"; fi
RUN_DIR="$EVAL_LIB_OUT_BASE/restore-lifecycle-$RUN_ID"
mkdir -p "$RUN_DIR" "$WORKSPACE_CWD"

SUCCESS=0
WORKSPACE_ID=""
TEAM_ID=""
PLAN_BASELINE="$RUN_DIR/plans-before.txt"

observe() { PYTHONIOENCODING=utf-8 PYTHONPATH="$SCRIPT_DIR" dev_run python3 "$SCRIPT_DIR/observe.py" "$@"; }
json_get() { python3 -c 'import json,sys; d=json.load(sys.stdin); print(d.get(sys.argv[1]) or "")' "$1"; }

cleanup() {
  if [ "$SUCCESS" = "1" ]; then
    echo "== cleanup $WORKSPACE_NAME"
    [ -z "$TEAM_ID" ] || dc team close "$TEAM_ID" --json > /dev/null 2>&1 || true
    [ -z "$WORKSPACE_ID" ] || dc workspace close "$WORKSPACE_ID" > /dev/null 2>&1 || true
    plan_sweep_new "$PLAN_BASELINE"
  else
    echo "== keeping $WORKSPACE_NAME id=${WORKSPACE_ID:-unknown} team=${TEAM_ID:-unknown} for forensics: $RUN_DIR"
  fi
}
trap cleanup EXIT

restart_dev() {
  local label="$1"
  echo "== [$label] save session"
  local saved
  saved="$(dev_post /api/session/save)" || { echo "FAIL [$label] session save not acknowledged"; exit 1; }
  [ "$(printf '%s' "$saved" | json_get ok)" = "True" ] || { echo "FAIL [$label] session save: $saved"; exit 1; }
  echo "== [$label] full dev stop (reboot analogue) and start"
  (
    cd "$REPO_ROOT" || exit 1
    bash build.sh kill
    bash build.sh dev-prebuilt --wait
  ) || { echo "FAIL [$label] dev restart"; exit 1; }
}

new_marker() { python3 -c 'import uuid; print("RESTORE-LIFECYCLE-" + uuid.uuid4().hex[:12])'; }

spawn_json() {
  local out
  out="$("$@" --json 2>&1)" || { echo "FAIL spawn: $* -> $out" >&2; return 1; }
  printf '%s' "$out"
}

dev_get /api/health > /dev/null || { echo "dev 7686 unavailable; start it with ./build.sh dev --wait" >&2; exit 7; }
plan_snapshot "$PLAN_BASELINE" || exit 1

echo "== create workspace $WORKSPACE_NAME"
WORKSPACE_ID="$(new_workspace "$WORKSPACE_NAME" "$WORKSPACE_CWD")"
[ -n "$WORKSPACE_ID" ] || { echo "FAIL could not create workspace"; exit 1; }

echo "== team pair: claude orchestrator + codex planner"
TEAMS_BEFORE="$(dc team list --json)"
PAIR="$(dc team pair --orchestrator "claude:$RL_CLAUDE_MODEL" --planner "codex:$RL_CODEX_MODEL" \
  --workspace "$WORKSPACE_ID" --title "$WORKSPACE_NAME" --json)" || { echo "FAIL team pair: $PAIR"; exit 1; }
ORCH="$(printf '%s' "$PAIR" | json_get orchestrator_tab_id)"
PLANNER="$(printf '%s' "$PAIR" | json_get planner_tab_id)"
[ -n "$ORCH" ] && [ -n "$PLANNER" ] || { echo "FAIL team pair returned no tabs: $PAIR"; exit 1; }
TEAM_ID="$(dc team list --json | python3 -c '
import json, sys
before = {t.get("team_id") or t.get("id") for t in json.loads(sys.argv[1] or "[]")}
after = json.load(sys.stdin)
print(next((t.get("team_id") or t.get("id") for t in after if (t.get("team_id") or t.get("id")) not in before), ""))
' "$TEAMS_BEFORE")"

echo "== spawn Team workers and standalone agents"
mkdir -p "$WORKSPACE_CWD/claude-worker" "$WORKSPACE_CWD/codex-worker" "$WORKSPACE_CWD/standalone" "$WORKSPACE_CWD/shell"
CLAUDE_WORKER="$(spawn_json dc --as-tab "$ORCH" agent spawn claude --role Worker --parent-tab "$ORCH" \
  --workspace "$WORKSPACE_ID" --cwd "$WORKSPACE_CWD/claude-worker" --title rl-claude-worker \
  --model "$RL_CLAUDE_MODEL" --non-user-facing | json_get tab_id)"
CODEX_WORKER="$(spawn_json dc --as-tab "$ORCH" agent spawn codex --role Worker --parent-tab "$ORCH" \
  --workspace "$WORKSPACE_ID" --cwd "$WORKSPACE_CWD/codex-worker" --title rl-codex-worker \
  --model "$RL_CODEX_MODEL" --non-user-facing | json_get tab_id)"
STANDALONE="$(spawn_json dc agent spawn claude --workspace "$WORKSPACE_ID" --cwd "$WORKSPACE_CWD/standalone" \
  --title rl-standalone --model "$RL_CLAUDE_MODEL" --non-user-facing | json_get tab_id)"
SHELL_TAB="$(new_shell_tab "$WORKSPACE_ID" "$WORKSPACE_CWD/shell" rl-shell)"
for id in "$CLAUDE_WORKER" "$CODEX_WORKER" "$STANDALONE" "$SHELL_TAB"; do
  [ -n "$id" ] || { echo "FAIL a member did not spawn"; exit 1; }
done

AGENTS=("$ORCH" "$PLANNER" "$CLAUDE_WORKER" "$CODEX_WORKER" "$STANDALONE")
ALL_TABS=("${AGENTS[@]}" "$SHELL_TAB")
PROVIDERS=("$ORCH=claude" "$PLANNER=codex" "$CLAUDE_WORKER=claude" "$CODEX_WORKER=codex" "$STANDALONE=claude")
printf 'orchestrator=%s planner=%s claude_worker=%s codex_worker=%s standalone=%s shell=%s team=%s\n' \
  "$ORCH" "$PLANNER" "$CLAUDE_WORKER" "$CODEX_WORKER" "$STANDALONE" "$SHELL_TAB" "$TEAM_ID" | tee "$RUN_DIR/tabs.txt"

echo "== bind every agent session and persist one turn each"
observe wait-bound "$RL_BIND_DEADLINE" "${AGENTS[@]}" || exit 1
turn_all() {
  local label="$1" pids=() tab marker
  for tab in "${AGENTS[@]}"; do
    marker="$(new_marker)"
    echo "$label $tab $marker" >> "$RUN_DIR/markers.txt"
    observe send-marker "$RL_TURN_DEADLINE" "$tab" "$marker" "Reply with exactly $marker and nothing else." &
    pids+=("$!")
  done
  local rc=0 pid
  for pid in "${pids[@]}"; do wait "$pid" || rc=1; done
  return "$rc"
}
turn_all baseline || { echo "FAIL baseline turn did not complete on every agent"; exit 1; }

observe capture "$RUN_DIR/identity-before.json" "${ALL_TABS[@]}" > /dev/null || exit 1
observe assert-pre "$RUN_DIR/identity-before.json" "$SHELL_TAB" "${PROVIDERS[@]}" || exit 1

for cycle in $(seq 1 "$RL_CYCLES"); do
  label="cycle-$cycle"
  if [ "$cycle" = "$RL_MIDTURN_CYCLE" ]; then
    echo "== [$label] start a long turn on the Claude worker and restart while it runs"
    observe send-long-turn "$RL_TURN_DEADLINE" "$CLAUDE_WORKER" \
      "Write the integers from 1 to 3000, one per line, as plain text. Do not use tools." || exit 1
  fi
  restart_since="$(python3 -c 'import time; print(time.time())')"
  restart_dev "$label"

  echo "== [$label] restore report observes every agent pane live"
  observe restore-report "$RL_REPORT_DEADLINE" "$restart_since" "${AGENTS[@]}" -- "$SHELL_TAB" || exit 1

  echo "== [$label] identity is unchanged behind the same tab ids"
  observe capture "$RUN_DIR/identity-$label.json" "${ALL_TABS[@]}" > /dev/null || exit 1
  observe assert-identity "$RUN_DIR/identity-before.json" "$RUN_DIR/identity-$label.json" "$SHELL_TAB" || exit 1

  echo "== [$label] every restored agent answers a new turn in its resumed session"
  turn_all "$label" || { echo "FAIL [$label] a restored agent did not answer"; exit 1; }
  observe capture "$RUN_DIR/identity-$label-after-turn.json" "${ALL_TABS[@]}" > /dev/null || exit 1
  observe assert-identity "$RUN_DIR/identity-before.json" "$RUN_DIR/identity-$label-after-turn.json" "$SHELL_TAB" || exit 1
done

echo "== restore lifecycle PASS cycles=$RL_CYCLES midturn_cycle=$RL_MIDTURN_CYCLE run=$RUN_DIR"
SUCCESS=1
