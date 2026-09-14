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
RL_OWNED_DEADLINE="${RL_OWNED_DEADLINE:-180}"
# A provider store that never owns a bound session is a setup verdict, not a restore FAIL.
SETUP_EXIT=3
RL_CLAUDE_MODEL="${RL_CLAUDE_MODEL:-haiku}"
RL_CODEX_MODEL="${RL_CODEX_MODEL:-}"

RUN_ID="$(python3 -c 'import uuid; print(uuid.uuid4().hex[:10])')"
WORKSPACE_NAME="RESTORE-LIFECYCLE-$RUN_ID"
RUN_DIR="$EVAL_LIB_OUT_BASE/restore-lifecycle-$RUN_ID"
mkdir -p "$RUN_DIR"

SUCCESS=0
WORKSPACE_ID=""
WORKSPACE_CWD=""
TEAM_ID=""
PLAN_BASELINE="$RUN_DIR/plans-before.txt"

observe() { PYTHONIOENCODING=utf-8 PYTHONPATH="$SCRIPT_DIR" dev_run python3 "$SCRIPT_DIR/observe.py" "$@"; }
fail() { echo "FAIL $*"; exit 1; }

# Run a JSON-emitting command; stdout must be one JSON document. stderr is kept
# beside it so a refusal or a non-JSON payload fails with both streams shown.
cli_json() {
  local label="$1"; shift
  local out err rc
  err="$RUN_DIR/$label.stderr"
  out="$("$@" 2> "$err")"
  rc=$?
  printf '%s' "$out" > "$RUN_DIR/$label.json"
  if [ "$rc" -ne 0 ]; then
    echo "FAIL [$label] exit=$rc: $*" >&2
    echo "  stdout: $out" >&2
    echo "  stderr: $(cat "$err")" >&2
    return 1
  fi
  printf '%s' "$out"
}

# json_get LABEL PATH: PATH is dot-separated keys; a missing or empty value fails
# with the raw payload instead of a traceback.
json_get() {
  local label="$1" path="$2"
  PYTHONIOENCODING=utf-8 python3 -c '
import json, sys
label, path = sys.argv[1], sys.argv[2]
raw = sys.stdin.read()
try:
    value = json.loads(raw)
except ValueError as error:
    sys.stderr.write(f"FAIL [{label}] not JSON ({error}); raw={raw[:2000]!r}\n")
    sys.exit(1)
for key in path.split("."):
    value = value.get(key) if isinstance(value, dict) else None
if value in (None, ""):
    sys.stderr.write(f"FAIL [{label}] missing {path}; raw={raw[:2000]!r}\n")
    sys.exit(1)
print(value if isinstance(value, str) else json.dumps(value))
' "$label" "$path"
}

# Pick a model the provider advertises: the requested one when listed, else the
# provider default. A requested model that is not listed refuses the run, since
# the daemon would silently fall back and the bench would test another model.
provider_model() {
  local provider="$1" requested="$2" providers
  providers="$(dev_get /api/agents/providers)" || return 1
  printf '%s' "$providers" > "$RUN_DIR/providers.json"
  PYTHONIOENCODING=utf-8 python3 -c '
import json, sys
provider, requested = sys.argv[1], sys.argv[2]
raw = sys.stdin.read()
try:
    entries = json.loads(raw)
except ValueError:
    sys.stderr.write(f"FAIL providers not JSON; raw={raw[:2000]!r}\n"); sys.exit(1)
if isinstance(entries, dict):
    entries = entries.get("providers", [])
entry = next((p for p in entries if isinstance(p, dict) and p.get("id") == provider), None)
if entry is None:
    sys.stderr.write(f"FAIL provider {provider} not registered\n"); sys.exit(1)
ids = [m.get("id") for m in entry.get("available_models", []) if isinstance(m, dict) and m.get("id")]
if requested:
    if ids and requested not in ids:
        sys.stderr.write(f"FAIL {provider} model {requested!r} is not advertised; available={ids}\n"); sys.exit(1)
    print(requested)
else:
    print(entry.get("default_model") or "")
' "$provider" "$requested" < "$RUN_DIR/providers.json"
}

# A provider may normalize an alias (haiku -> a full id), so the resolved id must contain the request.
assert_model() {
  local label="$1" expected="$2" actual="$3"
  [ -z "$expected" ] && return 0
  case "$(printf '%s' "$actual" | tr '[:upper:]' '[:lower:]')" in
    *"$(printf '%s' "$expected" | tr '[:upper:]' '[:lower:]')"*) return 0 ;;
  esac
  fail "[$label] resolved model '$actual', requested '$expected'"
}

# The forced close is only for the workspace this run created: its recorded id
# must still carry this run's unique name.
bench_workspace_is_ours() {
  [ -n "$WORKSPACE_ID" ] || return 1
  dev_get /api/workspaces | PYTHONIOENCODING=utf-8 python3 -c '
import json, sys
ws_id, ws_name = sys.argv[1], sys.argv[2]
raw = sys.stdin.read()
try:
    entries = json.loads(raw)
except ValueError:
    sys.exit(1)
if isinstance(entries, dict):
    entries = entries.get("workspaces", [])
sys.exit(0 if any(isinstance(w, dict) and w.get("id") == ws_id and w.get("name") == ws_name for w in entries) else 1)
' "$WORKSPACE_ID" "$WORKSPACE_NAME"
}

cleanup() {
  if [ "$SUCCESS" = "1" ]; then
    echo "== cleanup $WORKSPACE_NAME"
    # dc blanks CODETERM_TAB_ID, so the caller is never a member of the Team or workspace it closes.
    [ -z "$TEAM_ID" ] || dc team close "$TEAM_ID" --json > "$RUN_DIR/team-close.json" 2>&1 \
      || echo "WARN team close refused: $(cat "$RUN_DIR/team-close.json")"
    if bench_workspace_is_ours; then
      # Standalone agents outlive the Team close; --force acknowledges destroying them.
      dc workspace close "$WORKSPACE_ID" --force > "$RUN_DIR/workspace-close.txt" 2>&1 \
        || echo "WARN workspace close refused: $(cat "$RUN_DIR/workspace-close.txt")"
    elif [ -n "$WORKSPACE_ID" ]; then
      echo "WARN workspace $WORKSPACE_ID no longer carries name $WORKSPACE_NAME; not closing it"
    fi
    plan_sweep_new "$PLAN_BASELINE"
  else
    echo "== keeping $WORKSPACE_NAME id=${WORKSPACE_ID:-unknown} team=${TEAM_ID:-unknown} for forensics: $RUN_DIR"
  fi
}
trap cleanup EXIT

restart_dev() {
  local label="$1" saved
  echo "== [$label] save session"
  saved="$(dev_post /api/session/save)" || fail "[$label] session save not acknowledged"
  case "$(printf '%s' "$saved" | json_get "$label-save" ok)" in True|true) ;; *) fail "[$label] session save: $saved" ;; esac
  echo "== [$label] full dev stop (reboot analogue) and start"
  (
    cd "$REPO_ROOT" || exit 1
    bash build.sh kill
    bash build.sh dev-prebuilt --wait
  ) || fail "[$label] dev restart"
}

new_marker() { python3 -c 'import uuid; print("RESTORE-LIFECYCLE-" + uuid.uuid4().hex[:12])'; }

dev_get /api/health > /dev/null || { echo "dev 7686 unavailable; start it with ./build.sh dev --wait" >&2; exit 7; }
plan_snapshot "$PLAN_BASELINE" || exit 1

CLAUDE_MODEL="$(provider_model claude "$RL_CLAUDE_MODEL")" || exit 1
CODEX_MODEL="$(provider_model codex "$RL_CODEX_MODEL")" || exit 1
echo "== models claude=${CLAUDE_MODEL:-default} codex=${CODEX_MODEL:-default}"
claude_spec="claude${CLAUDE_MODEL:+:$CLAUDE_MODEL}"
codex_spec="codex${CODEX_MODEL:+:$CODEX_MODEL}"

echo "== create provisioned workspace $WORKSPACE_NAME"
IFS=$'\t' read -r WORKSPACE_ID WORKSPACE_CWD <<< "$(new_provisioned_workspace "$WORKSPACE_NAME")"
[ -n "$WORKSPACE_ID" ] && [ -n "$WORKSPACE_CWD" ] || fail "could not create provisioned workspace"
echo "== workspace $WORKSPACE_ID provisioned at $WORKSPACE_CWD"

echo "== team pair: claude orchestrator + codex planner"
PAIR="$(cli_json team-pair dc team pair --orchestrator "$claude_spec" --planner "$codex_spec" \
  --workspace "$WORKSPACE_ID" --title "$WORKSPACE_NAME" --json)" || exit 1
ORCH="$(printf '%s' "$PAIR" | json_get team-pair orchestrator_tab_id)" || exit 1
PLANNER="$(printf '%s' "$PAIR" | json_get team-pair planner_tab_id)" || exit 1
assert_model team-pair-orchestrator "$CLAUDE_MODEL" "$(printf '%s' "$PAIR" | json_get team-pair orchestrator_model 2>/dev/null)"
assert_model team-pair-planner "$CODEX_MODEL" "$(printf '%s' "$PAIR" | json_get team-pair planner_model 2>/dev/null)"

ORCH_STATUS="$(dev_get "/api/tabs/$ORCH/status")" || fail "orchestrator status unavailable"
TEAM_ID="$(printf '%s' "$ORCH_STATUS" | json_get orchestrator-status team_id)" || exit 1
TEAMS="$(cli_json team-list dc team list --json)" || exit 1
printf '%s' "$TEAMS" | PYTHONIOENCODING=utf-8 python3 -c '
import json, sys
team_id = sys.argv[1]
raw = sys.stdin.read()
teams = json.loads(raw).get("teams")
if not isinstance(teams, list) or not any(isinstance(t, dict) and team_id in (t.get("id"), t.get("team_id")) for t in teams):
    sys.stderr.write(f"FAIL [team-list] Team {team_id} absent; raw={raw[:2000]!r}\n"); sys.exit(1)
' "$TEAM_ID" || exit 1

echo "== spawn Team workers and standalone agents"
# No --cwd: every tab inherits the daemon-created workspace directory, the only cwd with trust provenance.
spawn_member() {
  local label="$1" expected_model="$2"; shift 2
  local out tab
  out="$(cli_json "$label" "$@" --json)" || return 1
  tab="$(printf '%s' "$out" | json_get "$label" tab_id)" || return 1
  assert_model "$label" "$expected_model" "$(printf '%s' "$out" | json_get "$label" model 2>/dev/null)"
  printf '%s' "$tab"
}
CLAUDE_WORKER="$(spawn_member spawn-claude-worker "$CLAUDE_MODEL" dc --as-tab "$ORCH" agent spawn claude \
  --role Worker --parent-tab "$ORCH" --workspace "$WORKSPACE_ID" \
  --title rl-claude-worker ${CLAUDE_MODEL:+--model "$CLAUDE_MODEL"} --non-user-facing)" || exit 1
CODEX_WORKER="$(spawn_member spawn-codex-worker "$CODEX_MODEL" dc --as-tab "$ORCH" agent spawn codex \
  --role Worker --parent-tab "$ORCH" --workspace "$WORKSPACE_ID" \
  --title rl-codex-worker ${CODEX_MODEL:+--model "$CODEX_MODEL"} --non-user-facing)" || exit 1
STANDALONE="$(spawn_member spawn-standalone "$CLAUDE_MODEL" dc agent spawn claude \
  --workspace "$WORKSPACE_ID" --title rl-standalone \
  ${CLAUDE_MODEL:+--model "$CLAUDE_MODEL"} --non-user-facing)" || exit 1
SHELL_OUT="$(cli_json spawn-shell dc tab new --workspace "$WORKSPACE_ID" --title rl-shell --json)" || exit 1
SHELL_TAB="$(printf '%s' "$SHELL_OUT" | json_get spawn-shell tab_id)" || exit 1

AGENTS=("$ORCH" "$PLANNER" "$CLAUDE_WORKER" "$CODEX_WORKER" "$STANDALONE")
ALL_TABS=("${AGENTS[@]}" "$SHELL_TAB")
PROVIDERS=("$ORCH=claude" "$PLANNER=codex" "$CLAUDE_WORKER=claude" "$CODEX_WORKER=codex" "$STANDALONE=claude")
printf 'orchestrator=%s planner=%s claude_worker=%s codex_worker=%s standalone=%s shell=%s team=%s\n' \
  "$ORCH" "$PLANNER" "$CLAUDE_WORKER" "$CODEX_WORKER" "$STANDALONE" "$SHELL_TAB" "$TEAM_ID" | tee "$RUN_DIR/tabs.txt"

echo "== bind every agent session and persist one turn each"
observe wait-bound "$RL_BIND_DEADLINE" "${AGENTS[@]}" || exit 1
turn_all() {
  local label="$1" pids=() tab marker rc=0 pid
  for tab in "${AGENTS[@]}"; do
    marker="$(new_marker)"
    echo "$label $tab $marker" >> "$RUN_DIR/markers.txt"
    observe send-marker "$RL_TURN_DEADLINE" "$tab" "$marker" "Reply with exactly $marker and nothing else." &
    pids+=("$!")
  done
  for pid in "${pids[@]}"; do wait "$pid" || rc=1; done
  return "$rc"
}
turn_all baseline || fail "baseline turn did not complete on every agent"

observe capture "$RUN_DIR/identity-before.json" "${ALL_TABS[@]}" > /dev/null || exit 1
observe assert-pre "$RUN_DIR/identity-before.json" "$SHELL_TAB" "${PROVIDERS[@]}" || exit 1

# Forensics only: a failed capture warns and never decides the verdict.
# --lifecycle inspects exactly one tab, so each tab is its own call.
capture_lifecycle() {
  local label="$1" tab parts="$RUN_DIR/lifecycle-$label.parts"
  rm -rf "$parts"
  mkdir -p "$parts"
  for tab in "${ALL_TABS[@]}"; do
    dc tab inspect --tab "$tab" --lifecycle --json > "$parts/$tab.json" 2> "$parts/$tab.stderr" \
      || echo "WARN [$label] lifecycle capture failed for tab $tab: $(cat "$parts/$tab.stderr")"
  done
  PYTHONIOENCODING=utf-8 python3 - "$parts" "$RUN_DIR/lifecycle-$label.json" "${ALL_TABS[@]}" <<'PY' \
    || echo "WARN [$label] lifecycle merge failed"
import json, pathlib, sys
parts, out, tabs = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), sys.argv[3:]
merged = {}
for tab in tabs:
    payload = parts / f"{tab}.json"
    stderr = parts / f"{tab}.stderr"
    raw = payload.read_text(encoding="utf-8", errors="replace") if payload.exists() else ""
    try:
        merged[tab] = json.loads(raw)
    except ValueError:
        detail = stderr.read_text(encoding="utf-8", errors="replace").strip() if stderr.exists() else ""
        merged[tab] = {"capture_error": detail or raw[:2000] or "empty output"}
out.write_text(json.dumps(merged, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
  rm -rf "$parts"
}

for cycle in $(seq 1 "$RL_CYCLES"); do
  label="cycle-$cycle"
  echo "== [$label] every provider store owns its captured session before the restart"
  observe wait-owned "$RL_OWNED_DEADLINE" "$RUN_DIR/identity-before.json" "$SHELL_TAB"
  owned_rc=$?
  if [ "$owned_rc" -eq "$SETUP_EXIT" ]; then
    capture_lifecycle "$label-setup"
    echo "== restore lifecycle SETUP cycle=$cycle (no restart taken; not a restore verdict) run=$RUN_DIR"
    exit "$SETUP_EXIT"
  fi
  [ "$owned_rc" -eq 0 ] || exit 1
  capture_lifecycle "$label-before"
  if [ "$cycle" = "$RL_MIDTURN_CYCLE" ]; then
    echo "== [$label] start a long turn on the Claude worker and restart while it runs"
    observe send-long-turn "$RL_TURN_DEADLINE" "$CLAUDE_WORKER" \
      "Write the integers from 1 to 3000, one per line, as plain text. Do not use tools." || exit 1
  fi
  restart_since="$(python3 -c 'import time; print(time.time())')"
  restart_dev "$label"

  echo "== [$label] restore report observes every agent pane live"
  observe restore-report "$RL_REPORT_DEADLINE" "$restart_since" "${AGENTS[@]}" -- "$SHELL_TAB"
  report_rc=$?
  capture_lifecycle "$label-after"
  [ "$report_rc" -eq 0 ] || exit 1

  echo "== [$label] identity is unchanged behind the same tab ids"
  observe capture "$RUN_DIR/identity-$label.json" "${ALL_TABS[@]}" > /dev/null || exit 1
  observe assert-identity "$RUN_DIR/identity-before.json" "$RUN_DIR/identity-$label.json" "$SHELL_TAB" || exit 1

  echo "== [$label] every restored agent answers a new turn in its resumed session"
  turn_all "$label" || fail "[$label] a restored agent did not answer"
  observe capture "$RUN_DIR/identity-$label-after-turn.json" "${ALL_TABS[@]}" > /dev/null || exit 1
  observe assert-identity "$RUN_DIR/identity-before.json" "$RUN_DIR/identity-$label-after-turn.json" "$SHELL_TAB" || exit 1
done

echo "== restore lifecycle PASS cycles=$RL_CYCLES midturn_cycle=$RL_MIDTURN_CYCLE run=$RUN_DIR"
SUCCESS=1
