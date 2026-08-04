#!/usr/bin/env bash
# Rename durability — rename must survive an immediate hard kill (no 30s autosave wait).
# Requires a live dev daemon on port 7686.
#
# Usage: bash evals/session-rename-durability/run.sh

set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"
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

echo "== verify last.json already has new names (no autosave wait)"
python - "$NEW_WS_NAME" "$NEW_TAB_TITLE" <<'PY'
import json, os, sys
ws_name, tab_title = sys.argv[1:3]
path = os.path.join(os.path.expanduser("~"), ".codeterm-dev", "sessions", "last.json")
data = json.load(open(path, encoding="utf-8"))
ws = next((w for w in data.get("workspaces", []) if w.get("name") == ws_name), None)
if not ws:
    print(f"FAIL: workspace {ws_name!r} not in last.json yet")
    sys.exit(1)
tab = next((t for t in ws.get("tabs", []) if t.get("title") == tab_title), None)
if not tab:
    print(f"FAIL: tab {tab_title!r} not in last.json yet")
    sys.exit(1)
print(f"  OK last.json has {ws_name!r} / {tab_title!r}")
PY

PRE=$(snapshot session-rename-pre)
echo "  pre snapshot: $PRE"

echo "== HARD KILL immediately (no autosave wait)"
dev_kill_hard
assert_atomic_save_clean || exit 1

echo "== restart dev"
dev_start_bg
dev_wait_ready 600 || exit 1
sleep 3

POST=$(snapshot session-rename-post)
echo "  post snapshot: $POST"

echo "== verify names survived restart"
python - "$PRE" "$POST" "$NEW_WS_NAME" "$NEW_TAB_TITLE" <<'PY'
import json, os, sys

pre_dir, post_dir, ws_name, tab_title = sys.argv[1:5]

def read_ws(path):
    with open(os.path.join(path, "ws.json"), encoding="utf-8") as f:
        return json.load(f)

pre_ws = read_ws(pre_dir)
post_ws = read_ws(post_dir)
pre = next((w for w in pre_ws if w.get("name") == ws_name), None)
post = next((w for w in post_ws if w.get("name") == ws_name), None)
if not pre:
    print(f"FAIL: pre snapshot missing workspace {ws_name!r}")
    sys.exit(1)
if not post:
    print(f"FAIL: workspace {ws_name!r} lost after restart")
    sys.exit(1)
if pre.get("id") != post.get("id"):
    print(f"FAIL: workspace id drift {pre.get('id')} -> {post.get('id')}")
    sys.exit(1)

with open(os.path.join(post_dir, "last.json"), encoding="utf-8") as f:
    last = json.load(f)
ws_last = next((w for w in last.get("workspaces", []) if w.get("name") == ws_name), None)
if not ws_last:
    print(f"FAIL: last.json missing {ws_name!r} after restart")
    sys.exit(1)
tab_last = next((t for t in ws_last.get("tabs", []) if t.get("title") == tab_title), None)
if not tab_last:
    print(f"FAIL: last.json missing tab {tab_title!r} after restart")
    sys.exit(1)

print(f"PASS: rename survived hard restart ({ws_name!r}, tab {tab_title!r})")
PY

cleanup_test_workspaces "WS-rename-"
cleanup_test_workspaces "WS-RENAMED-"
