#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=/dev/null
source "$ROOT/evals/_lib/lib.sh"

PROVIDER=""
SCENARIO=""
COUNT=1
RUN_INDEX=""
TIMEOUT="${STATE_CAPTURE_TIMEOUT:-180}"
OUT_ROOT="$ROOT/testbench/state-dumps"
PY="${PYTHON:-python3}"

usage() {
  cat >&2 <<'EOF'
usage: testbench/state-capture.sh --provider P --scenario S [--count N] [--run N]

Scenarios:
  working-idle      (alias: code-then-idle)
  multi-turn
  model-choice
  model-picker
  error

Writes numbered full-session dumps to:
  testbench/state-dumps/<provider>/<scenario>-<N>/

The script targets dev through evals/_lib dc only. If dc exits 7 it waits and
retries; this script never starts, kills, or rebuilds dev.
EOF
  exit 2
}

while [ $# -gt 0 ]; do
  case "$1" in
    --provider) PROVIDER="${2:-}"; shift 2 ;;
    --scenario) SCENARIO="${2:-}"; shift 2 ;;
    --count) COUNT="${2:-}"; shift 2 ;;
    --run) RUN_INDEX="${2:-}"; shift 2 ;;
    --timeout) TIMEOUT="${2:-}"; shift 2 ;;
    *) usage ;;
  esac
done

[ -n "$PROVIDER" ] && [ -n "$SCENARIO" ] || usage

case "$SCENARIO" in
  code-then-idle) SCENARIO="working-idle" ;;
  model_choice) SCENARIO="model-choice" ;;
  model_picker) SCENARIO="model-picker" ;;
esac

case "$SCENARIO" in
  working-idle|multi-turn|model-choice|model-picker|error) ;;
  *) usage ;;
esac

json_field() {
  "$PY" -c 'import json,sys
doc=json.load(sys.stdin)
for key in sys.argv[1].split("."):
    doc=doc[key]
print(doc)' "$1"
}

now_ms() { "$PY" -c 'import time; print(int(time.time()*1000))'; }

dc_retry() {
  local attempt=0
  while true; do
    set +e
    dc "$@"
    local code=$?
    set -e
    if [ "$code" -eq 0 ]; then
      return 0
    fi
    if [ "$code" -ne 7 ] || [ "$attempt" -ge 24 ]; then
      return "$code"
    fi
    attempt=$((attempt + 1))
    sleep 5
  done
}

dev_get_retry() {
  local path="$1" attempt=0
  while true; do
    set +e
    local body
    body="$(dev_get "$path")"
    local code=$?
    set -e
    if [ "$code" -eq 0 ] && [ -n "$body" ]; then
      printf '%s\n' "$body"
      return 0
    fi
    if [ "$attempt" -ge 24 ]; then
      return 7
    fi
    attempt=$((attempt + 1))
    sleep 5
  done
}

provider_version() {
  case "$PROVIDER" in
    claude) claude --version 2>/dev/null || true ;;
    codex) codex --version 2>/dev/null || true ;;
    cursor) cursor-agent --version 2>/dev/null || cursor --version 2>/dev/null || true ;;
    opencode) opencode --version 2>/dev/null || true ;;
    *) "$PROVIDER" --version 2>/dev/null || true ;;
  esac
}

pane_status() {
  dev_get_retry "/api/panes/$1/status" \
    | "$PY" -c "import json,sys; print(json.load(sys.stdin).get('status',''))" 2>/dev/null || true
}

pane_text() {
  dc_retry tab get-text --tab "$1" 2>/dev/null || true
}

spawn_agent() {
  local ws="$1" cwd="$2" title="$3"
  dc_retry agent spawn "$PROVIDER" --workspace "$ws" --cwd "$cwd" --title "$title" --json \
    | json_field pane_id
}

send_text() {
  local pane="$1" text="$2"
  dc_retry send --tab "$pane" "$text" --actions enter >/dev/null
}

send_actions() {
  local pane="$1" actions="$2"
  dc_retry send --tab "$pane" --actions "$actions" >/dev/null
}

rel_ms() {
  local start="$1"
  "$PY" - "$start" <<'PY'
import sys, time
print(max(0, int(time.time() * 1000) - int(sys.argv[1])))
PY
}

screen_kind() {
  "$PY" -c 'import re,sys
text=sys.stdin.read().lower()
option_lines=len(re.findall(r"(?m)^\s*(?:[›>❯→▸▶●•*-]|\d+[.)]|[a-z][.)]|\[[ xX]\])\s+\S", text))
option_lines += len(re.findall(r"(?m)(?:[›>❯→▸▶●•*-]\s*)?\[[ xX]\]\s+(?:\d+[.)]\s*)?\S", text))
option_lines += len(re.findall(r"(?m)(?:^|[┃│])\s*\d+[.)]\s+\S", text))
footer=any(s in text for s in ["enter to select","enter submit","esc to","esc dismiss","space select","tab select","type to filter","navigate"])
model_words=any(s in text for s in ["model", "provider", "gpt", "claude", "composer", "opencode"])
choice_words=any(s in text for s in ["choose", "choice", "option", "in-memory", "redis", "file-based", "migration"])
error_words=any(s in text for s in ["error", "failed", "failure", "not found", "no such file", "nonexistent_command_xyz", "command not found", "exit code"])
if "select model" in text and model_words:
    print("model_picker")
elif option_lines >= 2 and choice_words:
    print("model_choice")
elif option_lines >= 2 and (footer or model_words):
    print("model_picker")
elif error_words:
    print("error")
else:
    print("none")'
}

screen_idle() {
  "$PY" -c 'import sys
text=sys.stdin.read().lower()
idle_markers=[
    "add a follow-up",
    "plan, search, build anything",
    "send a message",
    "message opencode",
    "what can i do",
    "type your message",
    "ctrl+p commands",
]
active_markers=[
    "composing",
    "thinking",
    "globbing",
    "grepping",
    "reading",
    "editing",
    "running",
    "building",
    "tokens",
    "ctrl+c to stop",
]
idle=any(marker in text for marker in idle_markers)
active=any(marker in text for marker in active_markers)
print("yes" if idle and not active else "no")'
}

screen_is_question() {
  local pane="$1" subtype="$2"
  [ "$(pane_text "$pane" | screen_kind)" = "$subtype" ]
}

screen_is_error() {
  local pane="$1"
  [ "$(pane_text "$pane" | screen_kind)" = "error" ]
}

wait_status_seen() {
  local pane="$1" want="$2" deadline=$((SECONDS + TIMEOUT))
  while [ "$SECONDS" -lt "$deadline" ]; do
    [ "$(pane_status "$pane")" = "$want" ] && return 0
    sleep 1
  done
  return 1
}

wait_idle() {
  local pane="$1" deadline=$((SECONDS + TIMEOUT)) streak=0
  while [ "$SECONDS" -lt "$deadline" ]; do
    local st text kind idle
    st="$(pane_status "$pane")"
    text="$(pane_text "$pane")"
    kind="$(printf '%s' "$text" | screen_kind)"
    idle="$(printf '%s' "$text" | screen_idle)"
    if { [ "$st" = "Idle" ] || [ "$st" = "Waiting" ] || [ "$idle" = "yes" ]; } && { [ "$kind" = "none" ] || [ "$idle" = "yes" ]; }; then
      streak=$((streak + 1))
      [ "$streak" -ge 3 ] && return 0
    else
      streak=0
    fi
    sleep 1
  done
  return 1
}

settle_idle() {
  local pane="$1"
  wait_idle "$pane" || true
  sleep 1
}

wait_question() {
  local pane="$1" subtype="$2" deadline=$((SECONDS + TIMEOUT))
  while [ "$SECONDS" -lt "$deadline" ]; do
    screen_is_question "$pane" "$subtype" && return 0
    sleep 1
  done
  return 1
}

wait_error() {
  local pane="$1" deadline=$((SECONDS + TIMEOUT))
  while [ "$SECONDS" -lt "$deadline" ]; do
    screen_is_error "$pane" && return 0
    sleep 1
  done
  return 1
}

cast_ms() {
  local cast="$1"
  "$PY" - "$cast" <<'PY'
import json, sys
path = sys.argv[1]
last = 0
with open(path, encoding="utf-8", errors="replace") as f:
    next(f, None)
    for line in f:
        if line.strip():
            last = round(json.loads(line)[0] * 1000)
print(last)
PY
}

prompt_for() {
  local scenario="$1" run="$2"
  "$PY" - "$scenario" "$run" <<'PY'
import sys
scenario = sys.argv[1]
run = (int(sys.argv[2]) - 1) % 3
prompts = {
    "working-idle": [
        "Write a Python function fib(n) with a docstring in fib.py. Keep it small.",
        "Add input validation and two unit tests for fib.py.",
        "Refactor fib.py to use an iterative implementation; keep behavior.",
    ],
    "model-choice": [
        "Before implementing, ask me which caching strategy to use. Give exactly three numbered options: in-memory, Redis, or file-based, then wait for my choice.",
        "Ask me how to handle migrations as a multiple-choice question with exactly three numbered options, then stop and wait.",
        "Ask me to choose one API style from exactly three numbered options before doing any work, then wait for my answer.",
    ],
    "error": [
        "Run `nonexistent_command_xyz` and report the failure.",
        "Try to open and read /no/such/path, then report the error.",
        "Run a shell command that fails with exit code 7 and show the error.",
    ],
}
print(prompts[scenario][run])
PY
}

multi_prompt_for() {
  local run="$1" step="$2"
  "$PY" - "$run" "$step" <<'PY'
import sys
run = (int(sys.argv[1]) - 1) % 3
step = int(sys.argv[2])
sets = [
    ["Create greet.py with a hello function.", "Add a --name flag to greet.py.", "Write a small test for greet.py."],
    ["Create calc.py with an add(a, b) function.", "Add a subtract(a, b) function.", "Add a simple pytest file for calc.py."],
    ["Create notes.txt with one project note.", "Append a second note to notes.txt.", "Create README.md summarizing the notes."],
]
print(sets[run][step - 1])
PY
}

write_timeline() {
  local path="$1" steps_json="$2" segments_json="$3"
  "$PY" - "$path" "$steps_json" "$segments_json" <<'PY'
import json, sys
segments = []
for seg in json.loads(sys.argv[3]):
    if segments and segments[-1]["state"] == seg["state"] and segments[-1].get("subtype") == seg.get("subtype"):
        segments[-1]["to_ms"] = seg["to_ms"]
    else:
        segments.append(seg)
doc = {"steps": json.loads(sys.argv[2]), "segments": segments}
json.dump(doc, open(sys.argv[1], "w", encoding="utf-8"), indent=2)
open(sys.argv[1], "a", encoding="utf-8").write("\n")
PY
}

verify_dump() {
  local dir="$1" scenario="$2"
  "$PY" - "$dir" "$scenario" <<'PY'
import json, os, sys
dir, scenario = sys.argv[1], sys.argv[2]
errors = []
for name in ("session.cast", "timeline.json", "meta.json"):
    path = os.path.join(dir, name)
    if not os.path.exists(path):
        errors.append(f"{name} missing")
    elif os.path.getsize(path) < (200 if name == "session.cast" else 10):
        errors.append(f"{name} too small")
timeline_path = os.path.join(dir, "timeline.json")
if os.path.exists(timeline_path):
    timeline = json.load(open(timeline_path, encoding="utf-8"))
    segs = timeline.get("segments", [])
    labels = [(s.get("state"), s.get("subtype")) for s in segs]
    if any(int(s.get("to_ms", 0)) <= int(s.get("from_ms", 0)) for s in segs):
        errors.append("non-positive segment duration")
    if scenario == "working-idle":
        if [s for s, _ in labels] != ["working", "idle"]:
            errors.append(f"expected working,idle got {labels}")
    elif scenario == "multi-turn":
        states = [s for s, _ in labels]
        if len(states) < 6 or states[:6] != ["working", "idle", "working", "idle", "working", "idle"]:
            errors.append(f"expected 3 working/idle pairs got {labels}")
    elif scenario == "model-choice":
        if not any(s == "question" and t == "model_choice" for s, t in labels):
            errors.append(f"missing question:model_choice in {labels}")
        if labels[0][0] != "working" or labels[-1][0] != "idle":
            errors.append(f"expected working...idle got {labels}")
    elif scenario == "model-picker":
        if not any(s == "question" and t == "model_picker" for s, t in labels):
            errors.append(f"missing question:model_picker in {labels}")
        if labels[0][0] != "idle" or labels[-1][0] != "idle":
            errors.append(f"expected idle...idle got {labels}")
    elif scenario == "error":
        if not any(s == "error" for s, _ in labels):
            errors.append(f"missing error segment in {labels}")
if errors:
    print(f"dump verify failed: {dir}: " + "; ".join(errors), file=sys.stderr)
    sys.exit(1)
print(f"dump verify ok: {dir}")
PY
}

append_json_item() {
  "$PY" - "$1" "$2" <<'PY'
import json, sys
items=json.loads(sys.argv[1])
items.append(json.loads(sys.argv[2]))
print(json.dumps(items))
PY
}

run_capture() {
  local run="$1"
  local ws_cwd="${TMPDIR:-/tmp}/codeterm-state-capture-$PROVIDER-$SCENARIO-$run-$$"
  mkdir -p "$ws_cwd"
  cat > "$ws_cwd/fib.py" <<'EOF'
def fib(n):
    return n if n < 2 else fib(n - 1) + fib(n - 2)
EOF

  local out_dir="$OUT_ROOT/$PROVIDER/$SCENARIO-$run"
  mkdir -p "$out_dir"
  rm -f "$out_dir/session.cast" "$out_dir/timeline.json" "$out_dir/meta.json" "$out_dir/screen.txt" "$out_dir/session.screen.txt"

  local ws pane started_at steps segments
  ws="$(dc_retry workspace create "state-$PROVIDER-$SCENARIO-$run" --cwd "$ws_cwd" --json | json_field id)"
  pane="$(spawn_agent "$ws" "$ws_cwd" "state-$SCENARIO-$run")"
  sleep 5
  wait_idle "$pane" || true

  dc_retry tab capture start --tab "$pane" --out "$out_dir/session.cast" >/dev/null
  started_at="$(now_ms)"
  steps='[]'
  segments='[]'

  add_step() {
    local at="$1" action="$2" expect="$3"
    steps="$(append_json_item "$steps" "{\"at_ms\":$at,\"action\":\"$action\",\"expect\":\"$expect\"}")"
  }

  add_segment() {
    local from="$1" to="$2" state="$3" subtype="${4:-}"
    if [ -n "$subtype" ]; then
      segments="$(append_json_item "$segments" "{\"from_ms\":$from,\"to_ms\":$to,\"state\":\"$state\",\"subtype\":\"$subtype\"}")"
    else
      segments="$(append_json_item "$segments" "{\"from_ms\":$from,\"to_ms\":$to,\"state\":\"$state\"}")"
    fi
  }

  local t0 t1 t2 t3 t4 prompt answer
  case "$SCENARIO" in
    working-idle)
      prompt="$(prompt_for "$SCENARIO" "$run")"
      t0="$(rel_ms "$started_at")"
      send_text "$pane" "$prompt"
      add_step "$t0" "send coding task" "working"
      wait_status_seen "$pane" "Working" || true
      settle_idle "$pane"
      t1="$(rel_ms "$started_at")"
      add_step "$t1" "quiescence settled" "idle"
      add_segment "$t0" "$t1" "working"
      ;;
    multi-turn)
      t0="$(rel_ms "$started_at")"
      prompt="$(multi_prompt_for "$run" 1)"
      send_text "$pane" "$prompt"
      add_step "$t0" "send multi-turn task 1" "working"
      wait_status_seen "$pane" "Working" || true
      settle_idle "$pane"
      t1="$(rel_ms "$started_at")"
      add_step "$t1" "task 1 quiescence settled" "idle"
      add_segment "$t0" "$t1" "working"

      t2="$(rel_ms "$started_at")"
      prompt="$(multi_prompt_for "$run" 2)"
      send_text "$pane" "$prompt"
      add_step "$t2" "send multi-turn task 2" "working"
      wait_status_seen "$pane" "Working" || true
      settle_idle "$pane"
      t3="$(rel_ms "$started_at")"
      add_step "$t3" "task 2 quiescence settled" "idle"
      add_segment "$t1" "$t2" "idle"
      add_segment "$t2" "$t3" "working"

      t4="$(rel_ms "$started_at")"
      prompt="$(multi_prompt_for "$run" 3)"
      send_text "$pane" "$prompt"
      add_step "$t4" "send multi-turn task 3" "working"
      wait_status_seen "$pane" "Working" || true
      settle_idle "$pane"
      local t5
      t5="$(rel_ms "$started_at")"
      add_step "$t5" "task 3 quiescence settled" "idle"
      add_segment "$t3" "$t4" "idle"
      add_segment "$t4" "$t5" "working"
      ;;
    model-choice)
      prompt="$(prompt_for "$SCENARIO" "$run")"
      t0="$(rel_ms "$started_at")"
      send_text "$pane" "$prompt"
      add_step "$t0" "send prompt that asks for choices" "working"
      wait_status_seen "$pane" "Working" || true
      wait_question "$pane" "model_choice" || true
      t1="$(rel_ms "$started_at")"
      add_step "$t1" "choice block visible" "question:model_choice"
      answer="Use in-memory caching."
      case $(((run - 1) % 3)) in
        1) answer="Choose the first migration option." ;;
        2) answer="Use the REST API style." ;;
      esac
      send_text "$pane" "$answer"
      t2="$(rel_ms "$started_at")"
      add_step "$t2" "answer choice" "working"
      wait_status_seen "$pane" "Working" || true
      settle_idle "$pane"
      t3="$(rel_ms "$started_at")"
      add_step "$t3" "quiescence settled after answer" "idle"
      add_segment "$t0" "$t1" "working"
      add_segment "$t1" "$t2" "question" "model_choice"
      add_segment "$t2" "$t3" "working"
      ;;
    model-picker)
      t0="$(rel_ms "$started_at")"
      send_text "$pane" "/model"
      add_step "$t0" "send /model" "question:model_picker"
      wait_question "$pane" "model_picker" || true
      t1="$(rel_ms "$started_at")"
      add_step "$t1" "model picker visible" "question:model_picker"
      send_actions "$pane" "enter"
      t2="$(rel_ms "$started_at")"
      add_step "$t2" "accept model picker default" "idle"
      settle_idle "$pane"
      t3="$(rel_ms "$started_at")"
      add_step "$t3" "picker closed and idle" "idle"
      add_segment "$t0" "$t1" "idle"
      add_segment "$t1" "$t2" "question" "model_picker"
      add_segment "$t2" "$t3" "idle"
      ;;
    error)
      prompt="$(prompt_for "$SCENARIO" "$run")"
      t0="$(rel_ms "$started_at")"
      send_text "$pane" "$prompt"
      add_step "$t0" "send failing task" "working"
      wait_status_seen "$pane" "Working" || true
      wait_error "$pane" || true
      t1="$(rel_ms "$started_at")"
      add_step "$t1" "error visible" "error"
      settle_idle "$pane"
      t2="$(rel_ms "$started_at")"
      add_step "$t2" "quiescence settled after error" "idle"
      add_segment "$t0" "$t1" "working"
      add_segment "$t1" "$t2" "error"
      ;;
  esac

  sleep 2
  dc_retry tab capture stop --tab "$pane" >/dev/null || true
  [ -f "$out_dir/session.screen.txt" ] && mv "$out_dir/session.screen.txt" "$out_dir/screen.txt"
  [ -f "$out_dir/screen.txt" ] || pane_text "$pane" > "$out_dir/screen.txt"

  local end_ms last_state last_subtype
  end_ms="$(cast_ms "$out_dir/session.cast")"
  local wall_end_ms
  wall_end_ms="$(rel_ms "$started_at")"
  [ "$end_ms" -ge "$wall_end_ms" ] || end_ms="$wall_end_ms"
  last_state="$("$PY" - "$segments" <<'PY'
import json, sys
items=json.loads(sys.argv[1])
print(items[-1]["state"] if items else "idle")
PY
)"
  last_subtype="$("$PY" - "$segments" <<'PY'
import json, sys
items=json.loads(sys.argv[1])
print(items[-1].get("subtype", "") if items else "")
PY
)"
  local last_to
  last_to="$("$PY" - "$segments" <<'PY'
import json, sys
items=json.loads(sys.argv[1])
print(items[-1]["to_ms"] if items else 0)
PY
)"
  if [ "$end_ms" -gt "$last_to" ]; then
    if [ "$last_state" = "question" ]; then
      add_segment "$last_to" "$end_ms" "$last_state" "$last_subtype"
    else
      add_segment "$last_to" "$end_ms" "idle"
    fi
  fi

  write_timeline "$out_dir/timeline.json" "$steps" "$segments"

  local provider_version_value
  provider_version_value="$(provider_version | head -1 | sed 's/"/\\"/g')"
  cat > "$out_dir/meta.json" <<JSON
{
  "provider": "$PROVIDER",
  "scenario": "$SCENARIO",
  "run": $run,
  "provider_version": "$provider_version_value",
  "cli_version": "$provider_version_value",
  "capturedAtMs": $(now_ms),
  "source": "state-capture.sh dev-7686",
  "paneId": "$pane",
  "workspaceId": "$ws"
}
JSON

  dc_retry pane close "$pane" >/dev/null 2>&1 || true
  verify_dump "$out_dir" "$SCENARIO"
  echo "wrote $out_dir"
}

if [ -n "$RUN_INDEX" ]; then
  run_capture "$RUN_INDEX"
else
  for run in $(seq 1 "$COUNT"); do
    run_capture "$run"
  done
fi
