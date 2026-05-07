#!/usr/bin/env bash
# Shared bench helpers — sourceable. Cross-platform (mac/linux/windows-bash).
#
# Usage:
#   source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"
#   dev_env
#   snap_label=$(snapshot pre)
#   ...
#
# Goals: reduce per-bench shell ceremony so the orchestrator calling
# the bench from a Claude session burns fewer tool calls. Also keeps
# OS-specific commands (taskkill vs kill, named pipes vs sockets) in
# one place so individual benches stay portable.

set -u

EVAL_LIB_REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
EVAL_LIB_OUT_BASE="$EVAL_LIB_REPO_ROOT/target-check/evals"
mkdir -p "$EVAL_LIB_OUT_BASE"

is_windows() { case "$OSTYPE" in msys*|cygwin*|win32) return 0;; *) return 1;; esac; }
is_macos()   { case "$OSTYPE" in darwin*) return 0;; *) return 1;; esac; }
is_linux()   { case "$OSTYPE" in linux*) return 0;; *) return 1;; esac; }

# --- Dev daemon connection -------------------------------------------

# Read API token from ~/.codeterm-dev/config.toml
dev_token() {
  local cfg="$HOME/.codeterm-dev/config.toml"
  [ -f "$cfg" ] || { echo "ERROR: $cfg not found — is dev installed?" >&2; return 1; }
  grep -E '^token = ' "$cfg" | head -1 | sed 's/.*"\(.*\)"/\1/'
}

# Socket path differs across OS. Windows uses a named pipe; Unix uses
# a stable name in OS-managed namespace (GenericNamespaced).
dev_socket_path() {
  if is_windows; then
    echo '\\.\pipe\codeterm-dev.sock'
  else
    echo 'codeterm-dev.sock'
  fi
}

# Export the four env vars the codeterm CLI needs to target dev.
# After this, plain `codeterm ...` hits port 7686 with the right auth.
dev_env() {
  export CODETERM_API_PORT=7686
  export CODETERM_API_TOKEN="$(dev_token)" || return 1
  export CODETERM_SOCKET_PATH="$(dev_socket_path)"
  export CODETERM_DEV_AUTH_HEADER="Authorization: Bearer $CODETERM_API_TOKEN"
}

# Convenience: GET an API path, returns JSON. Requires dev_env first.
dev_get() {
  curl -s --max-time 10 -H "$CODETERM_DEV_AUTH_HEADER" "http://127.0.0.1:7686$1"
}

dev_post() {
  local path="$1"; shift
  curl -s --max-time 30 -X POST -H "$CODETERM_DEV_AUTH_HEADER" \
    -H "Content-Type: application/json" --data "${1:-{}}" \
    "http://127.0.0.1:7686$path"
}

# --- Lifecycle -------------------------------------------------------

# Returns dev PID on stdout if listening on 7686, empty otherwise.
dev_pid() {
  if is_windows; then
    netstat -ano 2>/dev/null | grep ":7686.*LISTEN" | awk '{print $5}' | head -1
  elif is_macos || is_linux; then
    lsof -ti:7686 2>/dev/null | head -1
  fi
}

dev_alive() { [ -n "$(dev_pid)" ]; }

# Hard-kill dev — simulates Windows shutdown, OOM kill, BSOD.
dev_kill_hard() {
  local pid; pid="$(dev_pid)"
  if [ -z "$pid" ]; then echo "(no dev to kill)"; return 0; fi
  if is_windows; then
    taskkill //PID "$pid" //F 2>&1 | tail -1
  else
    kill -9 "$pid" 2>&1
  fi
  sleep 2
}

# Polls /api/health until dev responds. Default timeout 120s.
dev_wait_ready() {
  local timeout="${1:-120}" elapsed=0
  while [ "$elapsed" -lt "$timeout" ]; do
    if curl -s --max-time 1 http://127.0.0.1:7686/api/health > /dev/null 2>&1; then
      return 0
    fi
    sleep 3
    elapsed=$((elapsed + 3))
  done
  echo "ERROR: dev not ready after ${timeout}s" >&2
  return 1
}

# Start dev in the background. Caller is responsible for waiting.
dev_start_bg() {
  ( cd "$EVAL_LIB_REPO_ROOT" && ./build.sh dev > /dev/null 2>&1 & )
}

# --- Snapshots -------------------------------------------------------

# Capture workspaces + panes + last.json under target-check/evals/<label>/.
# Echoes the snapshot dir on stdout so callers can chain.
snapshot() {
  local label="${1:?usage: snapshot <label>}"
  local dir="$EVAL_LIB_OUT_BASE/$label"
  mkdir -p "$dir"
  dev_get /api/workspaces > "$dir/ws.json"
  dev_get /api/panes      > "$dir/panes.json"
  python - "$dir" <<'PYEOF'
import os, sys, shutil
src = os.path.join(os.path.expanduser('~'), '.codeterm-dev', 'sessions', 'last.json')
if os.path.exists(src):
    shutil.copy(src, os.path.join(sys.argv[1], 'last.json'))
PYEOF
  echo "$dir"
}

# Print PASS/FAIL diff of two snapshot dirs. Exit 0 if all match, 1 if any drift.
# Compares: workspace ids/names/cwds/tab_counts AND pane ids/titles/agent_types/cwds.
diff_snapshots() {
  local pre="${1:?usage: diff_snapshots <pre_dir> <post_dir>}"
  local post="${2:?}"
  PYTHONIOENCODING=utf-8 python - "$pre" "$post" <<'PYEOF'
import json, sys, os
pre, post = sys.argv[1], sys.argv[2]
pre_ws = json.load(open(os.path.join(pre, 'ws.json')))
post_ws = json.load(open(os.path.join(post, 'ws.json')))
pre_p = {p['id']: p for p in json.load(open(os.path.join(pre, 'panes.json')))}
post_p = {p['id']: p for p in json.load(open(os.path.join(post, 'panes.json')))}

print(f'  workspaces: pre={len(pre_ws)} post={len(post_ws)}')
print(f'  panes:      pre={len(pre_p)} post={len(post_p)}')

fail = 0
for a in pre_ws:
    b = next((w for w in post_ws if w['id']==a['id']), None)
    if not b:
        print(f'  FAIL workspace MISSING after restart: {a["name"]} ({a["id"]})')
        fail += 1
        continue
    diffs = []
    if a.get('name') != b.get('name'): diffs.append(f"name: {a.get('name')!r}->{b.get('name')!r}")
    if a.get('cwd') != b.get('cwd'): diffs.append(f"cwd: {a.get('cwd')!r}->{b.get('cwd')!r}")
    if a.get('tab_count') != b.get('tab_count'): diffs.append(f"tabs: {a.get('tab_count')}->{b.get('tab_count')}")
    if diffs:
        print(f"  FAIL workspace {a['name']:20s} " + ', '.join(diffs))
        fail += 1

for pid, a in pre_p.items():
    b = post_p.get(pid)
    if not b:
        print(f'  FAIL pane LOST: {pid} title={a.get("title")!r} agent={a.get("agent_type")}')
        fail += 1
        continue
    diffs = []
    for k in ('title','agent_type','cwd'):
        if a.get(k) != b.get(k): diffs.append(f'{k}: {a.get(k)!r}->{b.get(k)!r}')
    if diffs:
        print(f'  FAIL pane {pid}: ' + '; '.join(diffs))
        fail += 1

if fail == 0: print('  PASS metadata diff')
else: print(f'  FAIL {fail} drift(s)'); sys.exit(1)
PYEOF
}

# --- Cleanup ---------------------------------------------------------

# Close every workspace whose name starts with prefix. Default 'WS-' so
# leftover test workspaces from prior runs don't pollute the next one.
cleanup_test_workspaces() {
  local prefix="${1:-WS-}"
  dev_get /api/workspaces | python -c "
import json, sys
for w in json.load(sys.stdin):
    if w['name'].startswith('$prefix'): print(w['id'])
" | while read -r id; do
    [ -n "$id" ] || continue
    codeterm workspace close "$id" > /dev/null 2>&1
  done
}

# Sanity check the on-disk session file is well-formed and that no
# half-written .tmp got left behind. Returns non-zero on either issue.
# Uses python's os.path.expanduser so the path resolves correctly on
# Windows (git bash $HOME is /c/Users/X — invalid for native Python).
assert_atomic_save_clean() {
  python <<'PYEOF' || { echo "  FAIL session file integrity check"; return 1; }
import json, os, sys
sess = os.path.join(os.path.expanduser('~'), '.codeterm-dev', 'sessions')
tmp = os.path.join(sess, 'last.json.tmp')
last = os.path.join(sess, 'last.json')
if os.path.exists(tmp):
    print("  FAIL leftover last.json.tmp — atomic write didn't complete")
    sys.exit(1)
try:
    json.load(open(last))
except Exception as e:
    print(f"  FAIL last.json: {e}")
    sys.exit(1)
print("  OK atomic save clean")
PYEOF
}

# --- One-shot scenario builders --------------------------------------

# Create a workspace + return its id on stdout. Args: name, cwd.
new_workspace() {
  local name="$1" cwd="$2"
  mkdir -p "$cwd" 2>/dev/null
  codeterm workspace create "$name" --cwd "$cwd" --json 2>/dev/null \
    | python -c "import json,sys; print(json.load(sys.stdin).get('id',''))"
}

# Spawn a plain shell pane in the given workspace. Echoes pane_id.
new_shell_pane() {
  local ws="$1" cwd="$2" title="${3:-shell}"
  codeterm pane new --workspace "$ws" --cwd "$cwd" --title "$title" --json 2>/dev/null \
    | python -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))"
}

# Spawn a claude pane in the given workspace. Echoes pane_id.
# Optional 4th arg = initial --task prompt.
new_claude_pane() {
  local ws="$1" cwd="$2" title="${3:-claude}" task="${4:-}"
  local args=( agent spawn claude --workspace "$ws" --cwd "$cwd" --title "$title" )
  [ -n "$task" ] && args+=( --task "$task" )
  codeterm "${args[@]}" 2>&1 | grep -oE "pane [a-f0-9]+" | awk '{print $2}' | head -1
}
