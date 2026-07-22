#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=/dev/null
source "$ROOT/evals/_lib/lib.sh"

PROVIDER=""
SITUATION=""
PROMPT=""
UNTIL=""
TIMEOUT="${TB_CAPTURE_TIMEOUT:-120}"
WS_CWD="${TMPDIR:-/tmp}/codeterm-snapshot-testbench"
WS_NAME="WS-snapshot-testbench"
CAPTURE_AGENT_ARGS="${CAPTURE_AGENT_ARGS:-}"

usage() {
  echo "usage: $0 --provider P --situation S --prompt TEXT --until REGEX" >&2
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --provider) PROVIDER="${2:-}"; shift 2 ;;
    --situation) SITUATION="${2:-}"; shift 2 ;;
    --prompt) PROMPT="${2:-}"; shift 2 ;;
    --until) UNTIL="${2:-}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown arg: $1" >&2; usage; exit 2 ;;
  esac
done

[ -n "$PROVIDER" ] && [ -n "$SITUATION" ] && [ -n "$PROMPT" ] && [ -n "$UNTIL" ] || {
  usage
  exit 2
}

mkdir -p "$WS_CWD"

now_ms() { python3 -c 'import time;print(int(time.time()*1000))'; }

json_field() {
  python3 -c 'import json,sys
doc=json.load(sys.stdin)
for key in sys.argv[1].split("."):
    doc=doc[key]
print(doc)' "$1"
}

dc_retry() {
  local attempt=0 rc=0 out
  while true; do
    set +e
    out="$(dc "$@" 2>&1)"
    rc=$?
    set -e
    if [ "$rc" -ne 7 ]; then
      printf '%s' "$out"
      return "$rc"
    fi
    attempt=$((attempt + 1))
    [ "$attempt" -lt 10 ] || {
      printf '%s\n' "$out" >&2
      return "$rc"
    }
    sleep 3
  done
}

regex_matches() {
  python3 -c 'import re,sys
pattern=sys.argv[1]
text=sys.stdin.read()
sys.exit(0 if re.search(pattern, text, re.MULTILINE | re.DOTALL) else 1)' "$UNTIL"
}

provider_version() {
  local bin="${1}"
  if ! command -v "$bin" >/dev/null 2>&1; then
    echo "unavailable: $bin not found"
    return 0
  fi
  local out rc
  set +e
  out="$("$bin" --version 2>&1)"
  rc=$?
  set -e
  out="$(printf '%s\n' "$out" | head -1)"
  if [ -n "$out" ]; then
    printf '%s' "$out"
  else
    printf 'unavailable: %s --version exited %s' "$bin" "$rc"
  fi
}

workspace_id() {
  dc_retry workspace create "$WS_NAME" --cwd "$WS_CWD" --json | json_field id
}

spawn_agent() {
  local ws="$1"
  local args=(agent spawn "$PROVIDER" --workspace "$ws" --cwd "$WS_CWD" --title "tb-$PROVIDER-$SITUATION" --json)
  if [ -n "$CAPTURE_AGENT_ARGS" ]; then
    # shellcheck disable=SC2206
    local extra=( $CAPTURE_AGENT_ARGS )
    args+=(--args -- "${extra[@]}")
  fi
  dc_retry "${args[@]}" | json_field pane_id
}

wait_pty() {
  local pane="$1" deadline=$((SECONDS + 60))
  while [ "$SECONDS" -lt "$deadline" ]; do
    local pty
    pty="$(dc_retry pane list --json | python3 -c 'import json,sys
pane=sys.argv[1]
for p in json.load(sys.stdin):
    if p.get("id")==pane:
        print(p.get("pty_id") or "")
        break' "$pane")"
    [ -n "$pty" ] && return 0
    sleep 1
  done
  return 1
}

OUT_DIR="$ROOT/testbench/snapshots/$PROVIDER/$SITUATION"
OUT_TMP="$OUT_DIR/.capture.$$"
mkdir -p "$OUT_DIR"
rm -rf "$OUT_TMP"
mkdir -p "$OUT_TMP"
cleanup() { rm -rf "$OUT_TMP"; }
trap cleanup EXIT

ws="$(workspace_id)"
pane="$(spawn_agent "$ws")"
echo "[snapshot-testbench] provider=$PROVIDER situation=$SITUATION pane=$pane"
wait_pty "$pane" || { echo "pane $pane did not attach a PTY" >&2; exit 1; }

dc_retry tab capture start --tab "$pane" --out "$OUT_TMP/raw.cast" >/dev/null
dc_retry send "$PROMPT" --tab "$pane" >/dev/null

matched="false"
deadline=$((SECONDS + TIMEOUT))
while [ "$SECONDS" -lt "$deadline" ]; do
  screen="$(dc_retry tab get-text --tab "$pane" || true)"
  if printf '%s' "$screen" | regex_matches; then
    matched="true"
    break
  fi
  sleep 1
done

dc_retry tab capture stop --tab "$pane" >/dev/null
[ "$matched" = "true" ] || { echo "regex did not match before timeout: $UNTIL" >&2; exit 1; }
[ -f "$OUT_TMP/raw.cast" ] || { echo "capture did not write $OUT_TMP/raw.cast" >&2; exit 1; }
[ -f "$OUT_TMP/raw.screen.txt" ] && mv "$OUT_TMP/raw.screen.txt" "$OUT_TMP/screen.txt"
[ -f "$OUT_TMP/screen.txt" ] || dc_retry tab get-text --tab "$pane" > "$OUT_TMP/screen.txt"

provider_version_text="$(provider_version "$PROVIDER")"
python3 - "$OUT_TMP/meta.json" "$PROVIDER" "$SITUATION" "$pane" "$UNTIL" "$PROMPT" "$provider_version_text" <<'PY'
import json, sys, time
path, provider, situation, pane, until, prompt, version = sys.argv[1:]
doc = {
    "provider": provider,
    "situation": situation,
    "capturedAtMs": int(time.time() * 1000),
    "paneId": pane,
    "source": "dev-7686",
    "until": until,
    "prompt": prompt,
    "providerCliVersion": version,
}
with open(path, "w", encoding="utf-8") as f:
    json.dump(doc, f, ensure_ascii=False, indent=2)
    f.write("\n")
PY

mv "$OUT_TMP/raw.cast" "$OUT_DIR/raw.cast"
mv "$OUT_TMP/screen.txt" "$OUT_DIR/screen.txt"
mv "$OUT_TMP/meta.json" "$OUT_DIR/meta.json"
echo "wrote $OUT_DIR"
