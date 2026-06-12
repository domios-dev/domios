#!/usr/bin/env bash
# Controlled single-pane probe: measure how long claude takes to persist a
# marker turn to its on-disk JSONL vs. render it on screen. Throwaway debug aid.
set -u
set -o pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
source "$REPO_ROOT/evals/_lib/lib.sh"

RUN="dbg$(date +%s 2>/dev/null || echo 0)"
WS_NAME="RM-DEBUG-$RUN"
CWD="C:/temp/$WS_NAME/rm-claude-1"
MARKER="RESTORE-MARKER-DEBUG-$RUN"
mkdir -p "$CWD"

echo "== create workspace $WS_NAME"
WS_ID=$(dc workspace create "$WS_NAME" --cwd "C:/temp/$WS_NAME" --json | python -c "import json,sys;print(json.load(sys.stdin)['id'])")
echo "ws=$WS_ID"

echo "== spawn claude pane (haiku)"
PANE=$(dc agent spawn claude --workspace "$WS_ID" --cwd "$CWD" --title rm-claude-1 --model haiku --json | python -c "import json,sys;print(json.load(sys.stdin)['pane_id'])")
echo "pane=$PANE"

echo "== send marker prompt"
curl -s --max-time 60 -X POST -H "$CODETERM_DEV_AUTH_HEADER" -H "Content-Type: application/json" \
  --data "$(python -c "import json,sys;print(json.dumps({'text':'Reply with exactly '+sys.argv[1]+' and nothing else.','enter':True}))" "$MARKER")" \
  "http://127.0.0.1:7686/api/panes/$PANE/send" > /dev/null
echo "sent at t=0"

PROJECTS="$HOME/.claude/projects"
START=$SECONDS
screen_hit=""
jsonl_hit=""
for i in $(seq 1 24); do
  t=$((SECONDS - START))
  if [ -z "$screen_hit" ]; then
    out=$(dc get-output --pane "$PANE" --lines 60 2>/dev/null || true)
    case "$out" in *"$MARKER"*) screen_hit="$t"; echo "[t=${t}s] SCREEN has marker";; esac
  fi
  if [ -z "$jsonl_hit" ]; then
    hit=$(grep -rl "$MARKER" "$PROJECTS"/*"$WS_NAME"*rm-claude-1/*.jsonl 2>/dev/null | head -1 || true)
    if [ -n "$hit" ]; then jsonl_hit="$t"; echo "[t=${t}s] JSONL has marker: $hit"; fi
  fi
  if [ -n "$screen_hit" ] && [ -n "$jsonl_hit" ]; then break; fi
  sleep 5
done

echo "== RESULT: screen=${screen_hit:-NEVER}s jsonl=${jsonl_hit:-NEVER}s (within $((SECONDS-START))s)"
echo "== jsonl dir listing:"
ls -la "$PROJECTS"/*"$WS_NAME"*rm-claude-1/ 2>/dev/null || echo "(no dir)"
echo "== closing workspace"
dc workspace close "$WS_ID" > /dev/null 2>&1 || true
