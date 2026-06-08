#!/usr/bin/env bash
# CLI-agent benchmark helpers. Sources the dev-locked _lib/lib.sh (dc, dev_get).
set -euo pipefail
HARNESS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$HARNESS_DIR/../.." && pwd)"
# shellcheck source=/dev/null
source "$REPO_ROOT/evals/_lib/lib.sh"   # auto-calls dev_env (port 7686 + token)

# Read a pane's parsed transcript as JSON: {messages:[{type,content,toolName,toolInput,toolResult}], count}
read_chat() {  # <pane_id> [since]
  dev_get "/api/panes/$1/chat?since=${2:-0}"
}

# Wait until the transcript stops growing for `quiet` seconds, or `timeout` total.
wait_idle() {  # <pane_id> <timeout_s> <quiet_s>
  local pane="$1" timeout="$2" quiet="$3" start last_count=-1 last_change now count
  start=$(date +%s); last_change=$start
  while :; do
    now=$(date +%s)
    count=$(read_chat "$pane" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("count",0))' 2>/dev/null || echo 0)
    if [ "$count" != "$last_count" ]; then last_count=$count; last_change=$now; fi
    [ $((now - last_change)) -ge "$quiet" ] && return 0
    [ $((now - start)) -ge "$timeout" ] && return 0
    sleep 2
  done
}
