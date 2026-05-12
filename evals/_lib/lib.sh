#!/usr/bin/env bash
# Shared bench helpers — sourceable. Cross-platform (mac/linux/windows-bash).
#
# CRITICAL — read once before using:
#
#   Every bench targets the DEV daemon (port 7686). The user's prod
#   daemon (port 7685) is NEVER touched by these helpers. To make
#   that impossible to break by accident:
#
#     1. Sourcing this file auto-calls dev_env (sets all four env
#        vars for the dev daemon). You don't need to call dev_env
#        yourself.
#     2. Use `dc <subcommand>` instead of `codeterm <subcommand>`.
#        `dc` re-exports the dev env-vars in its own subshell so a
#        leaked CODETERM_API_PORT=7685 from the parent pane (which
#        is what happens when a Claude session lives inside prod
#        codeterm) cannot route the call to prod.
#     3. The HTTP helpers (dev_get, dev_post) are hardcoded to
#        http://127.0.0.1:7686 — no env-var indirection.
#
# Usage:
#   source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"
#   dc workspace list           # safe — always dev
#   snap_label=$(snapshot pre)
#   reset_dev_state
#
# Cross-platform (mac/linux/windows-bash). Filesystem paths go through
# python's os.path.expanduser so git bash $HOME (/c/Users/X) doesn't
# break native Windows Python.

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

# Convenience: GET an API path, returns JSON. Hardcoded to dev (7686)
# so it CANNOT accidentally hit prod even if the parent shell's
# CODETERM_API_PORT is 7685.
dev_get() {
  curl -s --max-time 10 -H "$CODETERM_DEV_AUTH_HEADER" "http://127.0.0.1:7686$1"
}

dev_post() {
  local path="$1"; shift
  curl -s --max-time 30 -X POST -H "$CODETERM_DEV_AUTH_HEADER" \
    -H "Content-Type: application/json" --data "${1:-{}}" \
    "http://127.0.0.1:7686$path"
}

# Run the codeterm CLI against the dev daemon, regardless of what
# CODETERM_* vars the parent shell inherited. This is the ONLY way
# bench scripts should invoke the CLI — `codeterm <cmd>` directly
# follows the parent env, which in a prod-hosted Claude pane silently
# routes to port 7685 + the prod socket.
#
# Assert dev is alive BEFORE every call. Otherwise the CLI's socket
# auto-discovery falls back to prod's pipe when codeterm-dev.sock
# doesn't exist — which is exactly how Andrey lost prod state on
# 2026-05-07. Fail loud with non-zero exit instead of silently
# dispatching to the wrong daemon.
dc() {
  if ! curl -s --max-time 1 http://127.0.0.1:7686/api/health > /dev/null 2>&1; then
    echo "ERROR [dc]: dev daemon not on port 7686. Start it: ./build.sh dev" >&2
    echo "             (would have leaked to prod via socket-discovery fallback)" >&2
    return 7
  fi
  # Strip parent CODETERM_PANE_ID — would force a [from <prod-pane>] prefix on every send-text.
  CODETERM_API_PORT=7686 \
  CODETERM_API_TOKEN="$CODETERM_API_TOKEN" \
  CODETERM_SOCKET_PATH="$CODETERM_SOCKET_PATH" \
  CODETERM_PANE_ID= \
  CODETERM_PARENT_PANE_ID= \
  codeterm "$@"
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
    dc workspace close "$id" > /dev/null 2>&1
  done
}

# Reset dev to a clean baseline: closes every non-protected workspace
# (i.e. everything except General). Use at the START of a bench when
# the test isn't about restoring prior state.
#
# Two paths:
#   - reset_dev_state (light) — close via HTTP, with retries. Fast
#     when there's only a couple of leftover workspaces. Hits a
#     known backend bug where back-to-back close of *restored*
#     workspaces (PTYs already killed by the restart) intermittently
#     returns dropped connections; the retry loop covers the gap.
#   - reset_dev_state_hard (nuclear) — kills the daemon, blanks
#     last.json, restarts. Guarantees a single-workspace baseline
#     (just General) regardless of what state the daemon was in.
#     Use when the bench's `pre` snapshot count must be exact.
reset_dev_state() {
  local pass max_pass=4
  for pass in $(seq 1 $max_pass); do
    local ids
    ids=$(dev_get /api/workspaces | python -c "
import json, sys
for w in json.load(sys.stdin):
    if not w.get('protected', False):
        print(w['id'])
")
    [ -z "$ids" ] && return 0
    for id in $ids; do
      [ -n "$id" ] || continue
      dev_post "/api/workspaces/$id/close" > /dev/null 2>&1
      sleep 0.3
    done
  done
}

# Nuclear reset — kills dev, removes last.json (so restart can't
# restore old workspaces), restarts dev, waits for ready. Ends with
# exactly one workspace (General). Slow (~30s) but bulletproof.
reset_dev_state_hard() {
  dev_kill_hard
  rm -f "$(python -c "import os; print(os.path.join(os.path.expanduser('~'), '.codeterm-dev', 'sessions', 'last.json'))")"
  rm -rf "$(python -c "import os; print(os.path.join(os.path.expanduser('~'), '.codeterm-dev', 'sessions', 'backups'))")"
  dev_start_bg
  dev_wait_ready 180
  sleep 3
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
  dc workspace create "$name" --cwd "$cwd" --json 2>/dev/null \
    | python -c "import json,sys; print(json.load(sys.stdin).get('id',''))"
}

# Spawn a plain shell pane in the given workspace. Echoes pane_id.
new_shell_pane() {
  local ws="$1" cwd="$2" title="${3:-shell}"
  dc pane new --workspace "$ws" --cwd "$cwd" --title "$title" --json 2>/dev/null \
    | python -c "import json,sys; print(json.load(sys.stdin).get('pane_id',''))"
}

# Spawn a claude pane in the given workspace. Echoes pane_id.
# Optional 4th arg = initial --task prompt.
new_claude_pane() {
  local ws="$1" cwd="$2" title="${3:-claude}" task="${4:-}"
  local args=( agent spawn claude --workspace "$ws" --cwd "$cwd" --title "$title" )
  [ -n "$task" ] && args+=( --task "$task" )
  dc "${args[@]}" 2>&1 | grep -oE "pane [a-f0-9]+" | awk '{print $2}' | head -1
}

# Spawn a codex pane. Echoes pane_id. Optional 4th arg = --task prompt.
new_codex_pane() {
  local ws="$1" cwd="$2" title="${3:-codex}" task="${4:-}"
  local args=( agent spawn codex --workspace "$ws" --cwd "$cwd" --title "$title" )
  [ -n "$task" ] && args+=( --task "$task" )
  dc "${args[@]}" 2>&1 | grep -oE "pane [a-f0-9]+" | awk '{print $2}' | head -1
}

# Spawn an opencode pane. Echoes pane_id. Optional 4th arg = --task prompt.
new_opencode_pane() {
  local ws="$1" cwd="$2" title="${3:-opencode}" task="${4:-}"
  local args=( agent spawn opencode --workspace "$ws" --cwd "$cwd" --title "$title" )
  [ -n "$task" ] && args+=( --task "$task" )
  dc "${args[@]}" 2>&1 | grep -oE "pane [a-f0-9]+" | awk '{print $2}' | head -1
}

# --- Auto-init: source-time dev_env so callers can't forget. ---------
# Without this, a forgotten `dev_env` call early in a bench leaks the
# parent shell's CODETERM_API_PORT/SOCKET into `codeterm` calls — and
# when the parent is a Claude pane inside prod codeterm, every CLI
# call lands on prod (port 7685) instead of dev. Andrey's prod state
# got minor cleanup damage from exactly this on 2026-05-07.
dev_env > /dev/null 2>&1 || true
