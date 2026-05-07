#!/usr/bin/env bash
# Session-restore bench. Live test against dev daemon (port 7686).
# See ./README.md for the test plan and the bug it guards against.

set -u

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$REPO_ROOT/target-check/session-restore"
mkdir -p "$OUT/pre" "$OUT/post"

# --- Dev API auth from config ---
TOK=$(grep -E '^token = ' ~/.codeterm-dev/config.toml 2>/dev/null | head -1 | sed 's/.*"\(.*\)"/\1/')
if [ -z "$TOK" ]; then
  echo "ERROR: cannot read API token from ~/.codeterm-dev/config.toml" >&2
  exit 1
fi
DEV="CODETERM_API_PORT=7686 CODETERM_API_TOKEN=$TOK CODETERM_SOCKET_PATH=\\\\.\\pipe\\codeterm-dev.sock"

# --- Helpers ---
require_dev() {
  curl -s --max-time 2 -H "Authorization: Bearer $TOK" http://127.0.0.1:7686/api/health > /dev/null \
    || { echo "ERROR: dev daemon not on port 7686. Run ./build.sh dev first." >&2; exit 1; }
}
ws_id() {
  curl -s -H "Authorization: Bearer $TOK" http://127.0.0.1:7686/api/workspaces \
    | python -c "import json,sys; print(next((w['id'] for w in json.load(sys.stdin) if w['name']=='$1'), ''))"
}
wait_dev_ready() {
  until curl -s --max-time 1 http://127.0.0.1:7686/api/health > /dev/null 2>&1; do sleep 5; done
}

# --- 1. Cleanup stale ---
require_dev
echo "== cleanup stale WS-* leftover from prior runs =="
curl -s -H "Authorization: Bearer $TOK" http://127.0.0.1:7686/api/workspaces \
  | python -c "
import json, sys
for w in json.load(sys.stdin):
    if w['name'].startswith('WS-'):
        print(w['id'])" \
  | while read id; do eval "$DEV codeterm workspace close $id" 2>&1 > /dev/null; done

# --- 2. Create 6 scenarios ---
echo "== create 6 workspaces with mixed scenarios =="
for tc in WS-empty WS-shell-silent WS-shell-with-text WS-claude-silent WS-claude-with-text WS-claude-renamed; do
  cwd="C:/temp/restore-$tc"
  mkdir -p "$cwd" 2>/dev/null
  eval "$DEV codeterm workspace create '$tc' --cwd '$cwd' --json" 2>&1 > /dev/null
done

WS_E=$(ws_id WS-empty)
WS_SS=$(ws_id WS-shell-silent)
WS_ST=$(ws_id WS-shell-with-text)
WS_CS=$(ws_id WS-claude-silent)
WS_CT=$(ws_id WS-claude-with-text)
WS_CR=$(ws_id WS-claude-renamed)

P_SS=$(eval "$DEV codeterm pane new --workspace $WS_SS --cwd 'C:/temp/restore-WS-shell-silent' --title 'silent-shell' --json" 2>&1 | python -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))")

P_ST=$(eval "$DEV codeterm pane new --workspace $WS_ST --cwd 'C:/temp/restore-WS-shell-with-text' --title 'shell-with-text' --json" 2>&1 | python -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))")
sleep 2
eval "$DEV codeterm pane exec --pane $P_ST 'Get-Date; Get-Location'" 2>&1 > /dev/null

P_CS=$(eval "$DEV codeterm agent spawn claude --workspace $WS_CS --cwd 'C:/temp/restore-WS-claude-silent' --title 'silent-claude'" 2>&1 | grep -oE "pane [a-f0-9]+" | awk '{print $2}')

P_CT=$(eval "$DEV codeterm agent spawn claude --workspace $WS_CT --cwd 'C:/temp/restore-WS-claude-with-text' --title 'claude-with-text' --task 'reply with single word: PINEAPPLE'" 2>&1 | grep -oE "pane [a-f0-9]+" | awk '{print $2}')

P_CR=$(eval "$DEV codeterm agent spawn claude --workspace $WS_CR --cwd 'C:/temp/restore-WS-claude-renamed' --title 'will-rename'" 2>&1 | grep -oE "pane [a-f0-9]+" | awk '{print $2}')
eval "$DEV codeterm pane rename 'MY-CUSTOM-TITLE' --pane $P_CR" 2>&1 > /dev/null

cat > "$OUT/ids.txt" <<EOF
WS_E=$WS_E
WS_SS=$WS_SS P_SS=$P_SS
WS_ST=$WS_ST P_ST=$P_ST
WS_CS=$WS_CS P_CS=$P_CS
WS_CT=$WS_CT P_CT=$P_CT
WS_CR=$WS_CR P_CR=$P_CR
EOF
echo "== ids saved to $OUT/ids.txt =="
cat "$OUT/ids.txt"

# --- 3. Snapshot pre + wait autosave ---
echo "== wait 35s for autosave =="
sleep 35
curl -s -H "Authorization: Bearer $TOK" http://127.0.0.1:7686/api/workspaces > "$OUT/pre/ws.json"
curl -s -H "Authorization: Bearer $TOK" http://127.0.0.1:7686/api/panes > "$OUT/pre/panes.json"
cp ~/.codeterm-dev/sessions/last.json "$OUT/pre/last.json"

# --- 4. Hard kill ---
echo "== HARD KILL (taskkill /F — Windows shutdown simulator) =="
PID=$(netstat -ano 2>&1 | grep ":7686.*LISTEN" | awk '{print $5}' | head -1)
taskkill //PID "$PID" //F 2>&1 | tail -1
sleep 2

if [ -f ~/.codeterm-dev/sessions/last.json.tmp ]; then
  echo "FAIL: leftover last.json.tmp — atomic write didn't complete cleanly"
  exit 1
fi
python -c "import json; json.load(open(r'C:\Users\User\.codeterm-dev\sessions\last.json'))" 2>&1 \
  | grep -q "Error\|Traceback" && { echo "FAIL: last.json corrupt post-kill"; exit 1; }

# --- 5. Restart + wait ---
echo "== restart dev =="
( cd "$REPO_ROOT" && ./build.sh dev > /dev/null 2>&1 & )
wait_dev_ready
sleep 5

# --- 6. Snapshot post + verify ---
curl -s -H "Authorization: Bearer $TOK" http://127.0.0.1:7686/api/workspaces > "$OUT/post/ws.json"
curl -s -H "Authorization: Bearer $TOK" http://127.0.0.1:7686/api/panes > "$OUT/post/panes.json"

PYTHONIOENCODING=utf-8 python <<EOF
import json
pre = json.load(open(r'$OUT/pre/ws.json'))
post = json.load(open(r'$OUT/post/ws.json'))
pre_p = {p['id']: p for p in json.load(open(r'$OUT/pre/panes.json'))}
post_p = {p['id']: p for p in json.load(open(r'$OUT/post/panes.json'))}

print()
print(f'workspaces: pre={len(pre)} post={len(post)}')
print(f'panes:      pre={len(pre_p)} post={len(post_p)}')
print()

print('== workspace metadata diff')
fail = 0
for name in ('WS-empty','WS-shell-silent','WS-shell-with-text','WS-claude-silent','WS-claude-with-text','WS-claude-renamed'):
    a = next((w for w in pre if w['name']==name), None)
    b = next((w for w in post if w['name']==name), None)
    if not b: print(f'  FAIL MISSING: {name}'); fail += 1; continue
    diffs = []
    if a['id'] != b['id']: diffs.append(f"id: {a['id']}->{b['id']}")
    if a.get('cwd') != b.get('cwd'): diffs.append(f"cwd: {a.get('cwd')!r}->{b.get('cwd')!r}")
    if a.get('tab_count') != b.get('tab_count'): diffs.append(f"tabs: {a.get('tab_count')}->{b.get('tab_count')}")
    if diffs: print(f'  FAIL {name}: ' + ', '.join(diffs)); fail += 1
    else: print(f'  OK   {name}')

print()
print('== target pane diff (specific scenarios)')
import re
ids = open(r'$OUT/ids.txt').read()
def gid(label):
    m = re.search(rf'{label}=(\w+)', ids); return m.group(1) if m else ''
targets = [
    ('shell-silent', gid('P_SS')),
    ('shell-with-text', gid('P_ST')),
    ('claude-silent', gid('P_CS')),
    ('claude-with-text', gid('P_CT')),
    ('claude-renamed', gid('P_CR')),
]
for label, pid in targets:
    if pid not in post_p: print(f'  FAIL {label} pane={pid} LOST'); fail += 1; continue
    a = pre_p.get(pid, {})
    b = post_p[pid]
    diffs = []
    for k in ('title','agent_type','cwd'):
        if a.get(k) != b.get(k): diffs.append(f'{k}: {a.get(k)!r}->{b.get(k)!r}')
    if diffs: print(f'  FAIL {label}: ' + '; '.join(diffs)); fail += 1
    else: print(f'  OK   {label} pane={pid} title={b.get("title")!r} agent={b.get("agent_type")}')

print()
print('== UI-mount step is MANUAL: open dev window, click each WS-* tab,')
print('   then verify silent-claude / claude-with-text / claude-renamed all')
print('   show claude UI (not bare powershell), and silent-shell /')
print('   shell-with-text show plain powershell (no accidental claude).')
print()
if fail == 0: print('METADATA RESTORE: PASS')
else: print(f'METADATA RESTORE: FAIL ({fail} issues)'); exit(1)
EOF
