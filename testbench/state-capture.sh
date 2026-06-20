#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=/dev/null
source "$ROOT/evals/_lib/lib.sh"

PROVIDER=""
SCENARIO=""
TIMEOUT="${STATE_CAPTURE_TIMEOUT:-180}"
OUT_ROOT="$ROOT/testbench/state-dumps"
PY="${PYTHON:-python3}"

usage() {
  cat >&2 <<'EOF'
usage: testbench/state-capture.sh --provider P --scenario S

Scenarios:
  code-then-idle
  model-picker
  multi-turn
  approval

The script targets dev through evals/_lib dc only. If dc exits 7, start or wait
for dev and rerun; this script never starts, kills, or rebuilds dev.
EOF
  exit 2
}

while [ $# -gt 0 ]; do
  case "$1" in
    --provider) PROVIDER="${2:-}"; shift 2 ;;
    --scenario) SCENARIO="${2:-}"; shift 2 ;;
    --timeout) TIMEOUT="${2:-}"; shift 2 ;;
    *) usage ;;
  esac
done

[ -n "$PROVIDER" ] && [ -n "$SCENARIO" ] || usage

json_field() {
  "$PY" -c 'import json,sys
doc=json.load(sys.stdin)
for key in sys.argv[1].split("."):
    doc=doc[key]
print(doc)' "$1"
}

now_ms() { "$PY" -c 'import time; print(int(time.time()*1000))'; }

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
  dc pane get-text --pane "$1" 2>/dev/null || true
}

spawn_agent() {
  local ws="$1" cwd="$2" title="$3"
  dc agent spawn "$PROVIDER" --workspace "$ws" --cwd "$cwd" --title "$title" --json \
    | json_field pane_id
}

send_text() {
  local pane="$1" text="$2"
  dc send --pane "$pane" "$text" --actions enter >/dev/null
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
    local st
    st="$(pane_status "$pane")"
    if [ "$st" = "Idle" ] || [ "$st" = "Waiting" ]; then
      streak=$((streak + 1))
      [ "$streak" -ge 3 ] && return 0
    else
      streak=0
    fi
    sleep 1
  done
  return 1
}

screen_has_picker() {
  "$PY" -c 'import re,sys
text=sys.stdin.read().lower()
option_lines=len(re.findall(r"(?m)^\s*(?:[›>❯→▸▶●•*-]|\d+[.)]|[a-z][.)]|\[[ xX]\])\s+\S", text))
footer=any(s in text for s in ["enter to select","esc to","space select","tab select"])
words=any(s in text for s in ["model","provider","approve","allow","permission","login","update"])
print("yes" if option_lines >= 2 and (footer or words) else "no")'
}

wait_question() {
  local pane="$1" deadline=$((SECONDS + TIMEOUT))
  while [ "$SECONDS" -lt "$deadline" ]; do
    [ "$(pane_text "$pane" | screen_has_picker)" = "yes" ] && return 0
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
with open(path) as f:
    next(f, None)
    for line in f:
        if line.strip():
            last = round(json.loads(line)[0] * 1000)
print(last)
PY
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

WS_CWD="${TMPDIR:-/tmp}/codeterm-state-capture-$PROVIDER-$SCENARIO-$$"
mkdir -p "$WS_CWD"
cat > "$WS_CWD/fib.py" <<'EOF'
def fib(n):
    return n if n < 2 else fib(n - 1) + fib(n - 2)
EOF

OUT_DIR="$OUT_ROOT/$PROVIDER/$SCENARIO"
mkdir -p "$OUT_DIR"
rm -f "$OUT_DIR/session.cast" "$OUT_DIR/timeline.json" "$OUT_DIR/meta.json" "$OUT_DIR/screen.txt"

WS="$(dc workspace create "state-$PROVIDER-$SCENARIO" --cwd "$WS_CWD" --json | json_field id)"
PANE="$(spawn_agent "$WS" "$WS_CWD" "state-$SCENARIO")"
sleep 5

dc pane capture start --pane "$PANE" --out "$OUT_DIR/session.cast" >/dev/null

steps='[]'
segments='[]'
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
  segments="$("$PY" - "$segments" "$1" "$2" "$3" "${4:-}" <<'PY'
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

case "$SCENARIO" in
  code-then-idle)
    send_text "$PANE" "Edit fib.py so it uses an iterative implementation. Keep the file small."
    append_step 0 "send coding task" "working"
    wait_status_seen "$PANE" "Working" || true
    wait_idle "$PANE" || true
    ;;
  model-picker)
    send_text "$PANE" "/model"
    append_step 0 "send /model" "question:model_choice"
    wait_question "$PANE" || true
    send_text "$PANE" ""
    append_step "$(cast_ms "$OUT_DIR/session.cast")" "accept model picker default" "working"
    wait_idle "$PANE" || true
    ;;
  multi-turn)
    send_text "$PANE" "Create README.md with one sentence about this fake project."
    append_step 0 "send first coding task" "working"
    wait_status_seen "$PANE" "Working" || true
    wait_idle "$PANE" || true
    first_done="$(cast_ms "$OUT_DIR/session.cast")"
    send_text "$PANE" "Append one more sentence to README.md."
    append_step "$first_done" "send second coding task" "working"
    wait_status_seen "$PANE" "Working" || true
    wait_idle "$PANE" || true
    ;;
  approval)
    send_text "$PANE" "Run a command that lists this directory, asking approval if your CLI requires it."
    append_step 0 "send approval-provoking task" "question:permission"
    wait_question "$PANE" || true
    ;;
  *) usage ;;
esac

sleep 2
dc pane capture stop --pane "$PANE" >/dev/null || true
[ -f "$OUT_DIR/session.screen.txt" ] && mv "$OUT_DIR/session.screen.txt" "$OUT_DIR/screen.txt"
[ -f "$OUT_DIR/screen.txt" ] || pane_text "$PANE" > "$OUT_DIR/screen.txt"

end_ms="$(cast_ms "$OUT_DIR/session.cast")"
case "$SCENARIO" in
  model-picker|approval)
    append_segment 0 "$end_ms" "question" "$([ "$SCENARIO" = approval ] && echo permission || echo model_choice)"
    ;;
  multi-turn)
    mid=$((end_ms / 2))
    append_segment 0 "$mid" "working"
    append_segment "$mid" "$end_ms" "idle"
    ;;
  *)
    append_segment 0 "$end_ms" "working"
    ;;
esac
write_timeline "$OUT_DIR/timeline.json" "$steps" "$segments"

PROVIDER_VERSION="$(provider_version | head -1 | sed 's/"/\\"/g')"
cat > "$OUT_DIR/meta.json" <<JSON
{
  "provider": "$PROVIDER",
  "scenario": "$SCENARIO",
  "cli_version": "$PROVIDER_VERSION",
  "capturedAtMs": $(now_ms),
  "source": "state-capture.sh dev-7686",
  "paneId": "$PANE"
}
JSON

dc pane close "$PANE" >/dev/null 2>&1 || true
echo "wrote $OUT_DIR"
