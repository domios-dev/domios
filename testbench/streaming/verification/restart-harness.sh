#!/usr/bin/env bash
# Restart the ux-verifier live harness against the current dev daemon.
# Reads the dev token from the active dev data dir (it can change if the dev
# data dir is recreated) and reloads the matcher module from the worktree, so
# this is the one command to run after any dev restart or matcher commit.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
TOKEN="$(grep -E '^token = ' "$HOME/.codeterm-dev/config.toml" | head -1 | sed 's/.*"\(.*\)"/\1/')"
if [ -z "$TOKEN" ]; then
  echo "no dev token found in ~/.codeterm-dev/config.toml" >&2
  exit 1
fi
if ! curl -s --max-time 2 http://127.0.0.1:7686/api/health > /dev/null 2>&1; then
  echo "dev :7686 not healthy yet — not starting harness" >&2
  exit 7
fi
pkill -f ux-live-verify2 2>/dev/null
mkdir -p "$HERE/live"
nohup npx tsx "$HERE/ux-live-verify2.ts" --token "$TOKEN" --outdir "$HERE/live" \
  > "$HERE/live/harness.log" 2>&1 &
echo "harness restarted pid $! token ${TOKEN:0:6}… build $(curl -s --max-time 2 http://127.0.0.1:7686/api/health | python3 -c 'import json,sys; print(json.load(sys.stdin)["build"])' 2>/dev/null)"
