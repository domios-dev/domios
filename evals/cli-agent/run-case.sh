#!/usr/bin/env bash
# Usage: run-case.sh <case-file.yaml> <case-id> <provider> <runs-dir>
set -euo pipefail
HARNESS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=/dev/null
source "$HARNESS_DIR/lib.sh"

CASE_FILE="$1"; CASE_ID="$2"; PROVIDER="$3"; RUNS_DIR="$4"
mkdir -p "$RUNS_DIR"

read_field() {  # <field>
  python3 - "$CASE_FILE" "$CASE_ID" "$1" <<'PY'
import sys, yaml
cases = yaml.safe_load(open(sys.argv[1])) or []
c = next((x for x in cases if x.get("id") == sys.argv[2]), None)
print("" if c is None else (c.get(sys.argv[3], "") or ""))
PY
}

PROMPT="$(read_field prompt)"
[ -z "$PROMPT" ] && { echo "no prompt for $CASE_ID"; exit 2; }

TMP="$(mktemp -d)"
WS_JSON="$(dc workspace create "bench-$CASE_ID-$PROVIDER" --cwd "$TMP" --json 2>/dev/null || echo '{}')"
WS="$(echo "$WS_JSON" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("id",""))' 2>/dev/null || echo '')"
SPAWN="$(dc agent spawn "$PROVIDER" --workspace "$WS" --cwd "$TMP" --title "$CASE_ID" --json 2>/dev/null || echo '{}')"
PANE="$(echo "$SPAWN" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("pane_id",""))' 2>/dev/null || echo '')"

if [ -z "$PANE" ]; then
  echo "{\"case\":\"$CASE_ID\",\"provider\":\"$PROVIDER\",\"status\":\"SKIPPED\",\"reason\":\"spawn failed\"}" \
    > "$RUNS_DIR/$CASE_ID-$PROVIDER.json"
  [ -n "$WS" ] && dc workspace close "$WS" >/dev/null 2>&1 || true
  rm -rf "$TMP"
  echo "SKIPPED $CASE_ID/$PROVIDER (spawn failed)"
  exit 0
fi

sleep 8   # let the agent boot its REPL before typing
dc send-text "$PROMPT" --pane "$PANE" >/dev/null 2>&1 || true
wait_idle "$PANE" 180 15 90   # timeout=180s, quiet=15s, warmup(first-msg)=90s

read_chat "$PANE" > "$RUNS_DIR/$CASE_ID-$PROVIDER.json"
python3 "$HARNESS_DIR/transcript-md.py" "$RUNS_DIR/$CASE_ID-$PROVIDER.json" \
  "$CASE_ID" "$PROVIDER" "$PROMPT" > "$RUNS_DIR/$CASE_ID-$PROVIDER.md"

dc workspace close "$WS" >/dev/null 2>&1 || true
rm -rf "$TMP"
echo "wrote $RUNS_DIR/$CASE_ID-$PROVIDER.md"
