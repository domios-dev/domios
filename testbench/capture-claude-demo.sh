#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=/dev/null
source "$ROOT/evals/_lib/lib.sh"

OUT_ROOT="$ROOT/testbench/snapshots/claude"
WS_CWD="${TMPDIR:-/tmp}/codeterm-snapshot-testbench"
WS_NAME="WS-snapshot-testbench"
TIMEOUT="${TB_CAPTURE_TIMEOUT:-120}"
mkdir -p "$OUT_ROOT" "$WS_CWD"

now_ms() { python3 -c 'import time;print(int(time.time()*1000))'; }

json_field() {
  python3 -c 'import json,sys
doc=json.load(sys.stdin)
for key in sys.argv[1].split("."):
    doc=doc[key]
print(doc)' "$1"
}

workspace_id() {
  dc workspace create "$WS_NAME" --cwd "$WS_CWD" --json | json_field id
}

spawn_claude() {
  local ws="$1" situation="$2"
  dc agent spawn claude --workspace "$ws" --cwd "$WS_CWD" --title "tb-$situation" --json | json_field tab_id
}

wait_pty() {
  local tab="$1" deadline=$((SECONDS + 60))
  while [ "$SECONDS" -lt "$deadline" ]; do
    local pty
    pty="$(dc tab list --json | python3 -c 'import json,sys
tab=sys.argv[1]
for p in json.load(sys.stdin):
    if p.get("id")==tab:
        print(p.get("pty_id") or "")
        break' "$tab")"
    [ -n "$pty" ] && return 0
    sleep 1
  done
  return 1
}

chat_send() {
  local tab="$1" text="$2"
  python3 -c "import json,sys;print(json.dumps({'text':sys.argv[1],'enter':True}))" "$text" \
    | curl -s --max-time 30 -X POST -H "$CODETERM_DEV_AUTH_HEADER" \
        -H "Content-Type: application/json" --data @- \
        "http://127.0.0.1:7686/api/tabs/$tab/send" >/dev/null
}

text_has_options() {
  python3 -c 'import re,sys
text=sys.stdin.read()
lettered=len(re.findall(r"(?m)^\s*(?:[-*]\s*)?[ABC][.)]\s+\S", text))
numbered=len(re.findall(r"(?m)^\s*(?:[-*]\s*)?\d+[.)]\s+\S", text))
print("yes" if lettered >= 3 or numbered >= 3 else "no")'
}

capture_case() {
  local situation="$1" prompt="$2"
  local ws tab dir
  ws="$(workspace_id)"
  tab="$(spawn_claude "$ws" "$situation")"
  dir="$OUT_ROOT/$situation"
  mkdir -p "$dir"
  rm -f "$dir/raw.cast" "$dir/screen.txt"

  echo "[snapshot-testbench] $situation tab=$tab"
  wait_pty "$tab" || { echo "tab $tab did not attach a PTY" >&2; return 1; }
  dc tab capture start --tab "$tab" --out "$dir/raw.cast" >/dev/null
  chat_send "$tab" "$prompt"

  local deadline=$((SECONDS + TIMEOUT))
  while [ "$SECONDS" -lt "$deadline" ]; do
    local screen
    screen="$(dc tab get-text --tab "$tab" || true)"
    if [ "$(printf '%s' "$screen" | text_has_options)" = "yes" ]; then
      break
    fi
    sleep 3
  done
  dc tab capture stop --tab "$tab" >/dev/null
  [ -f "$dir/raw.cast" ] || { echo "capture did not write $dir/raw.cast" >&2; return 1; }
  [ -f "$dir/raw.screen.txt" ] && mv "$dir/raw.screen.txt" "$dir/screen.txt"
  [ -f "$dir/screen.txt" ] || dc tab get-text --tab "$tab" > "$dir/screen.txt"

  cat > "$dir/meta.json" <<JSON
{
  "provider": "claude",
  "situation": "$situation",
  "capturedAtMs": $(now_ms),
  "tabId": "$tab",
  "source": "dev-7686"
}
JSON
  echo "  wrote $dir"
}

capture_case "options-abc" "Show me a concise example of when you would give me a choice with exactly A/B/C options. Format it as one question followed by exactly three options labeled A), B), and C)."
capture_case "compound-wizard" "Create a compact multi-step setup wizard example. Include two sections, each with one question and exactly three choices labeled A), B), C). Use clear section headings."
capture_case "risk-tradeoff" "Give me a different decision prompt: one question about choosing a deployment strategy, followed by exactly three options labeled A), B), C), each with a short tradeoff."

echo "Run: $ROOT/testbench/run.sh"
