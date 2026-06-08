#!/usr/bin/env bash
# CLI-agent benchmark helpers. Sources the dev-locked _lib/lib.sh (dc, dev_get).
set -euo pipefail
HARNESS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$HARNESS_DIR/../.." && pwd)"
# shellcheck source=/dev/null
source "$REPO_ROOT/evals/_lib/lib.sh"   # auto-calls dev_env (port 7686 + token)

# Read a pane's parsed transcript as JSON. Tolerates errors / not-ready
# (session_id absent) by emitting an empty transcript instead of failing.
read_chat() {  # <pane_id> [since]
  local raw
  raw="$(dev_get "/api/panes/$1/chat?since=${2:-0}" 2>/dev/null || true)"
  case "$raw" in
    '{'*) printf '%s' "$raw" ;;
    *)    printf '{"messages":[],"count":0}' ;;
  esac
}

chat_count() {  # <pane_id>
  read_chat "$1" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("count",0))' 2>/dev/null || echo 0
}

# Wait for the agent to (a) produce its first message, then (b) go quiet.
# Phase 1: poll until count>0 or `warmup` elapses (agent boot + session_id +
# first response can take 20-40s). Phase 2: wait until count stops growing for
# `quiet` seconds, or `timeout` total. Returns 0 always (caller inspects the
# transcript; empty = no-response, judged FAIL).
wait_idle() {  # <pane_id> <timeout_s> <quiet_s> [<warmup_s>]
  local pane="$1" timeout="$2" quiet="$3" warmup="${4:-60}"
  local start now count last_count=0 last_change
  start=$(date +%s)
  # Phase 1: wait for first message.
  while :; do
    now=$(date +%s)
    count=$(chat_count "$pane")
    [ "$count" -gt 0 ] && break
    [ $((now - start)) -ge "$warmup" ] && return 0   # never started
    sleep 3
  done
  # Phase 2: idle detection.
  last_count=$count; last_change=$(date +%s)
  while :; do
    now=$(date +%s)
    count=$(chat_count "$pane")
    if [ "$count" != "$last_count" ]; then last_count=$count; last_change=$now; fi
    [ $((now - last_change)) -ge "$quiet" ] && return 0
    [ $((now - start)) -ge "$timeout" ] && return 0
    sleep 3
  done
}
