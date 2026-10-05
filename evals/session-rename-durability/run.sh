#!/usr/bin/env bash
# Rename durability — rename must survive an immediate hard kill (no 30s autosave wait).
# Requires a live daemon for the named dev instance.
# Set CODETERM_INSTANCE to the named dev instance before running.
#
# Usage: bash evals/session-rename-durability/run.sh

set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"
[ -n "${CODETERM_INSTANCE:-}" ] || {
  echo "ERROR: CODETERM_INSTANCE must name an isolated dev instance" >&2
  exit 7
}
dev_env || exit 1
dev_alive || { echo "ERROR: dev not running. Start it: ./build.sh dev" >&2; exit 1; }

if is_windows; then
  TEST_CWD="C:/temp/rename-durability"
else
  TEST_CWD="/tmp/rename-durability"
fi

cleanup_test_workspaces "WS-rename-"

echo "== create workspace + shell tab"
WS_ID=$(new_workspace "WS-rename-durability" "$TEST_CWD")
[ -n "$WS_ID" ] || { echo "FAIL: workspace create returned no id"; exit 1; }
TAB_ID=$(new_shell_tab "$WS_ID" "$TEST_CWD" "before-rename")
[ -n "$TAB_ID" ] || { echo "FAIL: tab create returned no id"; exit 1; }

TAB_ID=$(dev_get "/api/workspaces" | python -c "
import json, sys
ws_id = '$WS_ID'
for w in json.load(sys.stdin):
    if w['id'] == ws_id:
        print(w.get('active_tab_id') or (w.get('tabs') or [{}])[0].get('id',''))
        break
")
[ -n "$TAB_ID" ] || { echo "FAIL: could not resolve tab id"; exit 1; }

NEW_WS_NAME="WS-RENAMED-$(date +%s)"
NEW_TAB_TITLE="TAB-RENAMED-$(date +%s)"

assert_renamed_api() {
  dev_get /api/workspaces | python3 -c '
import json, sys
ws_name, tab_title = sys.argv[1:3]
workspaces = json.load(sys.stdin)
ws = next((w for w in workspaces if w.get("name") == ws_name), None)
if not ws:
    raise SystemExit(f"FAIL: workspace {ws_name!r} absent from dev API")
tabs = ws.get("tabs") or []
tab = next((t for t in tabs if t.get("title") == tab_title), None)
if not tab:
    raise SystemExit(f"FAIL: tab {tab_title!r} absent from dev API")
print(f"  OK API has {ws_name!r} / {tab_title!r}")
' "$NEW_WS_NAME" "$NEW_TAB_TITLE"
}

echo "== rename workspace + tab (daemon should persist immediately)"
dev_post "/api/workspaces/$WS_ID/rename" "{\"name\":\"$NEW_WS_NAME\"}" | python -c "
import json, sys
r = json.load(sys.stdin)
assert r.get('ok') is True or 'id' in r, r
" || { echo "FAIL: workspace rename API"; exit 1; }

dev_post "/api/workspaces/$WS_ID/tabs/$TAB_ID/rename" "{\"title\":\"$NEW_TAB_TITLE\"}" | python -c "
import json, sys
r = json.load(sys.stdin)
assert r.get('ok') is True, r
" || { echo "FAIL: tab rename API"; exit 1; }

echo "== verify names through the dev API"
assert_renamed_api || exit 1

PRE=$(snapshot session-rename-pre)
echo "  pre snapshot: $PRE"

echo "== HARD KILL immediately (no autosave wait)"
dev_kill_hard
assert_storage_root_present || exit 1

echo "== restart dev"
dev_start_bg
dev_wait_ready 600 || exit 1

POST=$(snapshot session-rename-post)
echo "  post snapshot: $POST"

echo "== verify names survived restart"
diff_snapshots "$PRE" "$POST" || exit 1
assert_renamed_api || exit 1

cleanup_test_workspaces "WS-rename-"
cleanup_test_workspaces "WS-RENAMED-"
