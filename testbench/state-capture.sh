#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=/dev/null
source "$ROOT/evals/_lib/lib.sh"

PROVIDER=""
SCENARIO=""
VARIANT=""
TIMEOUT="${STATE_CAPTURE_TIMEOUT:-240}"
OUT_ROOT="$ROOT/testbench/state-dumps"
PY="${PYTHON:-python3}"

usage() {
  cat >&2 <<'EOF'
usage: testbench/state-capture.sh --provider P --scenario S [--variant N] [--timeout SEC]

Scenarios:
  working-idle
  multi-turn
  model-choice
  model-picker
  error

Writes:
  testbench/state-dumps/<provider>/<scenario-N>/{session.cast,timeline.json,meta.json,screen.txt}

The script targets dev through evals/_lib dc only. If dc exits 7 it waits and retries.
It never starts, kills, or rebuilds dev.
EOF
  exit 2
}

while [ $# -gt 0 ]; do
  case "$1" in
    --provider) PROVIDER="${2:-}"; shift 2 ;;
    --scenario) SCENARIO="${2:-}"; shift 2 ;;
    --variant) VARIANT="${2:-}"; shift 2 ;;
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

dc_retry() {
  local attempt=0 rc
  while true; do
    set +e
    dc "$@"
    rc=$?
    set -e
    if [ "$rc" -eq 0 ]; then
      return 0
    fi
    if [ "$rc" -ne 7 ] || [ "$attempt" -ge 20 ]; then
      return "$rc"
    fi
    attempt=$((attempt + 1))
    sleep 6
  done
}

json_field() {
  "$PY" -c 'import json,sys
doc=json.load(sys.stdin)
for key in sys.argv[1].split("."):
    doc=doc[key]
print(doc)' "$1"
}

now_ms() { "$PY" -c 'import time; print(int(time.time()*1000))'; }
mark_ms() { echo "$(($(now_ms) - CAPTURE_START_MS))"; }

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
  dev_get "/api/panes/$1/status" \
    | "$PY" -c "import json,sys; print(json.load(sys.stdin).get('status',''))" 2>/dev/null || true
}

pane_text() {
  dc_retry pane get-text --pane "$1" 2>/dev/null || true
}

spawn_agent() {
  local ws="$1" cwd="$2" title="$3"
  dc_retry agent spawn "$PROVIDER" --workspace "$ws" --cwd "$cwd" --title "$title" --json \
    | json_field pane_id
}

send_text() {
  local pane="$1" text="$2"
  dc_retry send --pane "$pane" "$text" --actions enter >/dev/null
}

press_enter() {
  local pane="$1"
  dc_retry send --pane "$pane" --actions enter >/dev/null
}

json_string() {
  "$PY" -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$1"
}

capture_start() {
  local pane="$1" out="$2" out_json
  out_json="$(json_string "$out")"
  local body status
  body="$(mktemp)"
  printf '{"out":%s}' "$out_json" > "$body"
  status="$(curl -s --max-time 30 -o "$body.response" -w '%{http_code}' \
    -X POST -H "$CODETERM_DEV_AUTH_HEADER" -H "Content-Type: application/json" \
    --data-binary "@$body" "http://127.0.0.1:7686/api/panes/$pane/capture/start")"
  if [ "$status" != "200" ]; then
    cat "$body.response" >&2
    rm -f "$body" "$body.response"
    return 1
  fi
  rm -f "$body" "$body.response"
}

capture_stop() {
  local pane="$1"
  local body status
  body="$(mktemp)"
  status="$(curl -s --max-time 30 -o "$body.response" -w '%{http_code}' \
    -X POST -H "$CODETERM_DEV_AUTH_HEADER" -H "Content-Type: application/json" \
    --data '{}' "http://127.0.0.1:7686/api/panes/$pane/capture/stop")"
  if [ "$status" != "200" ]; then
    cat "$body.response" >&2
    rm -f "$body" "$body.response"
    return 1
  fi
  rm -f "$body" "$body.response"
}

cast_ms() {
  local cast="$1"
  "$PY" - "$cast" <<'PY'
import json, sys
last = 0
try:
    with open(sys.argv[1]) as f:
        next(f, None)
        for line in f:
            if line.strip():
                last = round(json.loads(line)[0] * 1000)
except FileNotFoundError:
    pass
print(last)
PY
}

screen_has_choice() {
  "$PY" -c 'import re,sys
text=sys.stdin.read().lower()
option_lines=len(re.findall(r"(?m)^\s*(?:[›>❯→▸▶●•*-]|\d+[.)]|[a-z][.)]|\[[ xX]\])\s+\S", text))
choice_words=any(s in text for s in ["choose", "choice", "select", "option", "which", "strategy", "caching", "migration"])
footer=any(s in text for s in ["enter to select", "esc to", "space select", "tab select"])
keyword_sets=[
    ["in-memory", "redis", "file-based"],
    ["reversible", "squash", "migration"],
    ["structured", "json", "logging"],
]
keyword_choice=any(sum(1 for k in ks if k in text) >= 2 for ks in keyword_sets)
print("yes" if (option_lines >= 2 and (choice_words or footer)) or keyword_choice else "no")'
}

screen_has_model_picker() {
  "$PY" -c 'import re,sys
text=sys.stdin.read().lower()
option_lines=len(re.findall(r"(?m)^\s*(?:[›>❯→▸▶●•*-]|\d+[.)]|[a-z][.)]|\[[ xX]\])\s+\S", text))
has_model=any(s in text for s in ["model", "reasoning", "provider"])
footer=any(s in text for s in ["enter to select", "esc to", "select model"])
print("yes" if option_lines >= 2 and has_model and (footer or "model" in text) else "no")'
}

screen_has_error() {
  "$PY" -c 'import sys
text=sys.stdin.read().lower()
needles=["nonexistent_command_xyz", "command not found", "not found", "no such file", "failed", "error", "exit code"]
print("yes" if any(n in text for n in needles) else "no")'
}

wait_status_seen() {
  local pane="$1" want="$2" deadline=$((SECONDS + TIMEOUT))
  while [ "$SECONDS" -lt "$deadline" ]; do
    [ "$(pane_status "$pane")" = "$want" ] && return 0
    sleep 1
  done
  return 1
}

wait_idle_mark() {
  local pane="$1" deadline=$((SECONDS + TIMEOUT)) streak=0 mark=0
  while [ "$SECONDS" -lt "$deadline" ]; do
    local st
    st="$(pane_status "$pane")"
    if [ "$st" = "Idle" ] || [ "$st" = "Waiting" ]; then
      streak=$((streak + 1))
      [ "$mark" -eq 0 ] && mark="$(mark_ms)"
      if [ "$streak" -ge 3 ]; then
        echo "$mark"
        return 0
      fi
    else
      streak=0
      mark=0
    fi
    sleep 1
  done
  return 1
}

wait_initial_idle() {
  local pane="$1" deadline=$((SECONDS + TIMEOUT)) streak=0
  while [ "$SECONDS" -lt "$deadline" ]; do
    local st pty
    st="$(pane_status "$pane")"
    pty="$(dev_get /api/panes | "$PY" -c 'import json,sys
pane=sys.argv[1]
for p in json.load(sys.stdin):
    if p.get("id")==pane:
        print(p.get("pty_id") or "")
        break' "$pane")"
    if { [ "$st" = "Idle" ] || [ "$st" = "Waiting" ]; } && [ -n "$pty" ]; then
      streak=$((streak + 1))
      [ "$streak" -ge 3 ] && return 0
    else
      streak=0
    fi
    sleep 1
  done
  return 1
}

wait_screen_mark() {
  local pane="$1" detector="$2" deadline=$((SECONDS + TIMEOUT))
  while [ "$SECONDS" -lt "$deadline" ]; do
    if [ "$(pane_text "$pane" | "$detector")" = "yes" ]; then
      mark_ms
      return 0
    fi
    sleep 1
  done
  return 1
}

pick_prompt() {
  local index="$1"; shift
  local count=$#
  local pos=$(( (index - 1) % count + 1 ))
  eval "printf '%s\n' \"\${$pos}\""
}

next_variant() {
  local base="$OUT_ROOT/$PROVIDER" max=0 name n
  mkdir -p "$base"
  for path in "$base"/"$SCENARIO"-*; do
    [ -d "$path" ] || continue
    name="$(basename "$path")"
    n="${name##*-}"
    case "$n" in
      ''|*[!0-9]*) ;;
      *) [ "$n" -gt "$max" ] && max="$n" ;;
    esac
  done
  echo $((max + 1))
}

write_timeline() {
  local path="$1" steps_json="$2" segments_json="$3"
  "$PY" - "$path" "$steps_json" "$segments_json" <<'PY'
import json, sys
doc = {"steps": json.loads(sys.argv[2]), "segments": json.loads(sys.argv[3])}
json.dump(doc, open(sys.argv[1], "w"), indent=2)
open(sys.argv[1], "a").write("\n")
PY
}

append_step() {
  steps="$("$PY" - "$steps" "$1" "$2" "$3" <<'PY'
import json, sys
steps=json.loads(sys.argv[1])
steps.append({"at_ms": int(sys.argv[2]), "action": sys.argv[3], "expect": sys.argv[4]})
print(json.dumps(steps))
PY
)"
}

append_segment() {
  local from="$1" to="$2" state="$3" subtype="${4:-}"
  [ "$to" -gt "$from" ] || to=$((from + 250))
  segments="$("$PY" - "$segments" "$from" "$to" "$state" "$subtype" <<'PY'
import json, sys
segments=json.loads(sys.argv[1])
seg={"from_ms": int(sys.argv[2]), "to_ms": int(sys.argv[3]), "state": sys.argv[4]}
if sys.argv[5]:
    seg["subtype"] = sys.argv[5]
segments.append(seg)
print(json.dumps(segments))
PY
)"
}

verify_timeline() {
  "$PY" - "$SCENARIO" "$OUT_DIR/session.cast" "$OUT_DIR/timeline.json" <<'PY'
import json, sys
scenario, cast_path, path = sys.argv[1], sys.argv[2], sys.argv[3]
doc = json.load(open(path))
segments = doc.get("segments", [])
states = [s.get("state") for s in segments]
subtypes = [s.get("subtype") for s in segments]
errors = []
try:
    rows = sum(1 for line in open(cast_path) if line.strip())
except FileNotFoundError:
    rows = 0
if rows < 5:
    errors.append(f"session.cast too thin or missing: {rows} rows")
if not segments:
    errors.append("no segments")
if any(int(s["to_ms"]) <= int(s["from_ms"]) for s in segments):
    errors.append("non-positive segment")
if scenario == "working-idle":
    if states[:2] != ["working", "idle"]:
        errors.append(f"expected working,idle got {states}")
elif scenario == "multi-turn":
    alternating = all(states[i] != states[i-1] for i in range(1, len(states)))
    if len(segments) < 4 or not alternating or states[0] != "working" or states[-1] != "idle":
        errors.append(f"expected >=4 alternating working/idle ending idle got {states}")
elif scenario == "model-choice":
    if "question" not in states or "model_choice" not in subtypes:
        errors.append(f"expected question:model_choice got {list(zip(states, subtypes))}")
    if states[:4] != ["working", "question", "working", "idle"]:
        errors.append(f"expected working,question,working,idle got {states}")
elif scenario == "model-picker":
    if "question" not in states or "model_choice" not in subtypes:
        errors.append(f"expected picker question:model_choice got {list(zip(states, subtypes))}")
elif scenario == "error":
    if "error" not in subtypes:
        errors.append(f"expected question:error got {list(zip(states, subtypes))}")
if errors:
    print("; ".join(errors), file=sys.stderr)
    sys.exit(1)
print("verified")
PY
}

VARIANT="${VARIANT:-$(next_variant)}"
OUT_DIR="$OUT_ROOT/$PROVIDER/$SCENARIO-$VARIANT"
WS_CWD="${TMPDIR:-/tmp}/codeterm-state-capture-$PROVIDER-$SCENARIO-$VARIANT-$$"
mkdir -p "$WS_CWD" "$OUT_DIR"
rm -f "$OUT_DIR/session.cast" "$OUT_DIR/timeline.json" "$OUT_DIR/meta.json" "$OUT_DIR/screen.txt"

cat > "$WS_CWD/fib.py" <<'EOF'
def fib(n):
    return n if n < 2 else fib(n - 1) + fib(n - 2)
EOF
cat > "$WS_CWD/main.py" <<'EOF'
def total(items):
    value = 0
    for item in items:
        value += item
    return value
EOF

steps='[]'
segments='[]'
PANE=""
cleanup() {
  if [ -n "$PANE" ]; then
    capture_stop "$PANE" 2>/dev/null || true
    dc_retry pane close "$PANE" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

WS="$(dc_retry workspace create "state-$PROVIDER-$SCENARIO-$VARIANT" --cwd "$WS_CWD" --json | json_field id)"
PANE="$(spawn_agent "$WS" "$WS_CWD" "state-$SCENARIO-$VARIANT")"
wait_initial_idle "$PANE" || true

capture_start "$PANE" "$OUT_DIR/session.cast"
CAPTURE_START_MS="$(now_ms)"
sleep 1

case "$SCENARIO" in
  working-idle)
    prompt="$(pick_prompt "$VARIANT" \
      "Write a Python function fib(n) with a docstring in fib.py. Keep it compact." \
      "Add input validation and two small unit tests for fib.py." \
      "Refactor fib.py to use an iterative implementation while keeping behavior.")"
    start="$(mark_ms)"
    send_text "$PANE" "$prompt"
    append_step "$start" "send coding task variant $VARIANT" "working"
    wait_status_seen "$PANE" "Working" || true
    idle="$(wait_idle_mark "$PANE")"
    append_step "$idle" "quiescence settled" "idle"
    end="$((idle + 2500))"
    ;;
  multi-turn)
    prompts=(
      "Create greet.py with a hello(name) function that returns a greeting."
      "Add a --name command-line flag to greet.py."
      "Write a small pytest test file for greet.py."
    )
    for i in 0 1 2; do
      start="$(mark_ms)"
      send_text "$PANE" "${prompts[$i]}"
      append_step "$start" "send multi-turn task $((i + 1))" "working"
      wait_status_seen "$PANE" "Working" || true
      idle="$(wait_idle_mark "$PANE")"
      append_step "$idle" "task $((i + 1)) quiescence settled" "idle"
      append_segment "$start" "$idle" "working"
      if [ "$i" -lt 2 ]; then
        sleep 1
        next_start="$(mark_ms)"
        append_segment "$idle" "$next_start" "idle"
      else
        end="$((idle + 2500))"
        append_segment "$idle" "$end" "idle"
      fi
    done
    ;;
  model-choice)
    prompt="$(pick_prompt "$VARIANT" \
      "Ask me to choose a caching strategy. Print exactly three numbered options: 1. in-memory, 2. Redis, 3. file-based. Then wait for my answer and do nothing else." \
      "Ask me how to handle migrations. Print exactly three numbered options: 1. reversible, 2. squash, 3. manual SQL. Then wait for my answer." \
      "Ask me to choose a logging approach. Print exactly three numbered options: 1. structured JSON, 2. plain text, 3. syslog. Then wait for my answer.")"
    answer="$(pick_prompt "$VARIANT" "Use in-memory caching." "Use reversible migrations." "Use structured JSON logging.")"
    start="$(mark_ms)"
    send_text "$PANE" "$prompt"
    append_step "$start" "send prompt that asks for model choice" "working"
    wait_status_seen "$PANE" "Working" || true
    question="$(wait_screen_mark "$PANE" screen_has_choice)"
    append_step "$question" "choice block visible" "question:model_choice"
    answer_at="$(mark_ms)"
    send_text "$PANE" "$answer"
    append_step "$answer_at" "answer model choice" "working"
    wait_status_seen "$PANE" "Working" || true
    idle="$(wait_idle_mark "$PANE")"
    append_step "$idle" "quiescence settled after answer" "idle"
    end="$((idle + 2500))"
    append_segment "$start" "$question" "working"
    append_segment "$question" "$answer_at" "question" "model_choice"
    append_segment "$answer_at" "$idle" "working"
    append_segment "$idle" "$end" "idle"
    ;;
  model-picker)
    start="$(mark_ms)"
    send_text "$PANE" "/model"
    append_step "$start" "send /model" "question:model_choice"
    question="$(wait_screen_mark "$PANE" screen_has_model_picker)"
    append_step "$question" "model picker visible" "question:model_choice"
    answer_at="$(mark_ms)"
    press_enter "$PANE"
    append_step "$answer_at" "accept model picker default" "idle"
    idle="$(wait_idle_mark "$PANE")"
    append_step "$idle" "picker closed and idle" "idle"
    end="$((idle + 2000))"
    append_segment "$start" "$question" "idle"
    append_segment "$question" "$answer_at" "question" "model_choice"
    append_segment "$answer_at" "$end" "idle"
    ;;
  error)
    prompt="$(pick_prompt "$VARIANT" \
      "Run nonexistent_command_xyz in the terminal and stop after showing the failure." \
      "Try to execute nonexistent_command_xyz and report the exact error output." \
      "Use the shell to run nonexistent_command_xyz; do not fix it, just show the failure.")"
    start="$(mark_ms)"
    send_text "$PANE" "$prompt"
    append_step "$start" "send failing-command task" "working"
    wait_status_seen "$PANE" "Working" || true
    err_at="$(wait_screen_mark "$PANE" screen_has_error)"
    append_step "$err_at" "error text visible" "question:error"
    idle="$(wait_idle_mark "$PANE" || mark_ms)"
    append_step "$idle" "post-error quiescence" "idle"
    end="$((idle + 2500))"
    append_segment "$start" "$err_at" "working"
    append_segment "$err_at" "$idle" "question" "error"
    append_segment "$idle" "$end" "idle"
    ;;
esac

sleep 2
capture_stop "$PANE" || true
[ -f "$OUT_DIR/session.screen.txt" ] && mv "$OUT_DIR/session.screen.txt" "$OUT_DIR/screen.txt"
[ -f "$OUT_DIR/screen.txt" ] || pane_text "$PANE" > "$OUT_DIR/screen.txt"

actual_end="$(cast_ms "$OUT_DIR/session.cast")"
if [ "${end:-0}" -gt "$actual_end" ]; then
  end="$actual_end"
fi
if [ "$SCENARIO" = "working-idle" ]; then
  append_segment "$start" "$idle" "working"
  append_segment "$idle" "${end:-$actual_end}" "idle"
fi

write_timeline "$OUT_DIR/timeline.json" "$steps" "$segments"

PROVIDER_VERSION="$(provider_version | head -1 | sed 's/"/\\"/g')"
cat > "$OUT_DIR/meta.json" <<JSON
{
  "provider": "$PROVIDER",
  "scenario": "$SCENARIO",
  "variant": $VARIANT,
  "cli_version": "$PROVIDER_VERSION",
  "capturedAtMs": $(now_ms),
  "source": "state-capture.sh dev-7686",
  "paneId": "$PANE"
}
JSON

verify_timeline >/dev/null
dc_retry pane close "$PANE" >/dev/null 2>&1 || true
PANE=""
echo "wrote $OUT_DIR"
