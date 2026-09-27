#!/usr/bin/env bash
# Shared bench helpers — sourceable. Cross-platform (mac/linux/windows-bash).
#
# CRITICAL — read once before using:
#
#   Every bench targets a DEV daemon. The user's prod
#   daemon (port 7685) is NEVER touched by these helpers. To make
#   that impossible to break by accident:
#
#     1. Sourcing this file sets NO CODETERM_* variable in your shell.
#        It resolves the dev connection into lib-private state and
#        composes those variables per call, into the one child process
#        that needs them. A shell that ran a bench is left exactly as
#        it was, so a later bare `codeterm` still means what it meant.
#     2. Use `dc <subcommand>` instead of `codeterm <subcommand>`.
#        `dc` composes the dev env-vars into the CLI's own process so a
#        leaked CODETERM_API_PORT=7685 from the parent tab (which
#        is what happens when a Claude session lives inside prod
#        codeterm) cannot route the call to prod. It also executes
#        the active dev CLI by absolute path, never through PATH.
#     3. The HTTP helpers use the selected dev instance port and token.
#     4. A child process that reads the connection from its environment
#        runs under `dev_run <cmd...>`; shell code reads `dev_auth_header`
#        and `dev_data_dir` instead of an inherited variable.
#
# Usage:
#   source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"
#   dc workspace list           # safe — always dev
#   dev_run python3 probe.py    # child sees dev; this shell does not
#   snap_label=$(snapshot pre)
#   reset_dev_state
#
# Cross-platform (mac/linux/windows-bash). Filesystem paths go through
# python3's os.path.expanduser so git bash $HOME (/c/Users/X) doesn't
# break native Windows Python.

set -u

EVAL_LIB_REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
EVAL_LIB_OUT_BASE="$EVAL_LIB_REPO_ROOT/target-check/evals"
mkdir -p "$EVAL_LIB_OUT_BASE"

is_windows() { case "$OSTYPE" in msys*|cygwin*|win32) return 0;; *) return 1;; esac; }
is_macos()   { case "$OSTYPE" in darwin*) return 0;; *) return 1;; esac; }
is_linux()   { case "$OSTYPE" in linux*) return 0;; *) return 1;; esac; }

# --- Dev daemon connection -------------------------------------------

EVAL_LIB_REQUESTED_INSTANCE="${CODETERM_INSTANCE:-}"
EVAL_LIB_REQUESTED_API_PORT="${CODETERM_API_PORT:-}"
EVAL_LIB_DEV_API_PORT=7686
EVAL_LIB_DEV_HOST=127.0.0.1
EVAL_LIB_RESOLVED_DATA_DIR=""
EVAL_LIB_RESOLVED_SOCKET_PATH=""
EVAL_LIB_RESOLVED_INSTANCE=""
EVAL_LIB_RESOLVED_API_TOKEN=""

dev_default_data_dir() {
  printf '%s\n' "$HOME/.codeterm-dev"
}

dev_requested_data_dir() {
  local instance slug
  instance="${EVAL_LIB_REQUESTED_INSTANCE:-}"
  instance="$(printf '%s' "$instance" | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')"
  if [ -z "$instance" ]; then
    dev_default_data_dir
    return
  fi
  slug="$(printf '%s' "$instance" | tr '[:upper:]' '[:lower:]' | sed 's/[^a-z0-9-]/-/g')"
  printf '%s\n' "$HOME/.codeterm-dev-$slug"
}

dev_resolution_error() {
  local resolved_path="$1" expected_data_dir="$2"
  printf "ERROR [dev-resolution]: instance='%s' resolved_path='%s' expected_data_dir='%s'\n" \
    "${EVAL_LIB_REQUESTED_INSTANCE:-default}" "$resolved_path" "$expected_data_dir" >&2
}

dev_bootstrap_cli() {
  local candidate data_dir
  data_dir="$(dev_requested_data_dir)"
  if [ -n "${CODETERM_CLI_PATH:-}" ] && [ -x "$CODETERM_CLI_PATH" ]; then
    printf '%s\n' "$CODETERM_CLI_PATH"
    return
  fi
  for candidate in \
    "$data_dir/bin/codeterm" \
    "$data_dir/bin/codeterm.exe"; do
    if [ -x "$candidate" ]; then
      printf '%s\n' "$candidate"
      return
    fi
  done
  dev_resolution_error '<unresolved>' "$data_dir"
  return 7
}

dev_shell_path() {
  if is_windows; then
    cygpath -u "$1"
  else
    printf '%s\n' "$1"
  fi
}

config_token() {
  grep -E '^token = ' "$1" | head -1 | sed 's/.*"\(.*\)"/\1/'
}

dev_resolve_identity() {
  local cli output parsed data_dir
  cli="$(dev_bootstrap_cli)" || return 7
  if ! output="$(
    unset CODETERM_API_TOKEN CODETERM_TAB_ID CODETERM_PARENT_TAB_ID
    if [ -n "$EVAL_LIB_REQUESTED_INSTANCE" ]; then
      export CODETERM_INSTANCE="$EVAL_LIB_REQUESTED_INSTANCE"
      unset CODETERM_SOCKET_PATH
      if [ -n "$EVAL_LIB_REQUESTED_API_PORT" ] &&
         [ "$EVAL_LIB_REQUESTED_API_PORT" != 7685 ]; then
        export CODETERM_API_PORT="$EVAL_LIB_REQUESTED_API_PORT"
      else
        unset CODETERM_API_PORT
      fi
    else
      unset CODETERM_INSTANCE
      export CODETERM_API_PORT=7686
      export CODETERM_SOCKET_PATH="$(dev_socket_path)"
    fi
    "$cli" identify --json
  )"; then
    dev_resolution_error '<unresolved>' '<identify --json unavailable>'
    return 7
  fi
  if ! parsed="$(printf '%s\n' "$output" | python3 -c '
import json, sys
connection = json.load(sys.stdin)["connection"]
values = [
    connection["data_dir"],
    connection["host"],
    str(connection["port"]),
    connection["socket_path"],
    connection.get("instance") or "",
]
print("\t".join(values))
')"; then
    dev_resolution_error '<unresolved>' '<valid identify --json connection>'
    return 7
  fi
  IFS=$'\t' read -r data_dir EVAL_LIB_DEV_HOST EVAL_LIB_DEV_API_PORT \
    EVAL_LIB_RESOLVED_SOCKET_PATH EVAL_LIB_RESOLVED_INSTANCE <<<"$parsed"
  EVAL_LIB_RESOLVED_DATA_DIR="$(dev_shell_path "$data_dir")"
  if [ "$EVAL_LIB_DEV_API_PORT" = 7685 ]; then
    dev_resolution_error "$EVAL_LIB_RESOLVED_DATA_DIR" "$EVAL_LIB_RESOLVED_DATA_DIR"
    return 7
  fi
}

dev_data_dir() {
  [ -n "$EVAL_LIB_RESOLVED_DATA_DIR" ] || dev_resolve_identity || return 7
  printf '%s\n' "$EVAL_LIB_RESOLVED_DATA_DIR"
}

# Read the API token from the active dev instance.
# The data dir comes from resolution, never from an inherited variable: a
# parent naming prod's dir would otherwise hand back prod's token.
dev_token() {
  local cfg data_dir
  data_dir="$(dev_data_dir)" || return 7
  cfg="$data_dir/config.toml"
  if [ ! -f "$cfg" ]; then
    dev_resolution_error "$cfg" "$data_dir"
    return 7
  fi
  config_token "$cfg"
}

# Socket path differs across OS. Windows uses a named pipe; Unix uses
# a stable name in OS-managed namespace (GenericNamespaced).
dev_socket_path() {
  if [ -n "$EVAL_LIB_RESOLVED_SOCKET_PATH" ]; then
    printf '%s\n' "$EVAL_LIB_RESOLVED_SOCKET_PATH"
    return
  fi
  if is_windows; then
    echo '\\.\pipe\codeterm-dev.sock'
  else
    echo 'codeterm-dev.sock'
  fi
}

# Resolve the dev connection into lib-private state. Exports nothing, so a
# shell that sourced this file keeps whatever CODETERM_* it already had.
# CLI selection remains dc's responsibility and never resolves through PATH.
dev_resolve() {
  local token
  if [ -n "$EVAL_LIB_RESOLVED_DATA_DIR" ] && [ -n "$EVAL_LIB_RESOLVED_API_TOKEN" ]; then
    return 0
  fi
  dev_resolve_identity || return 7
  EVAL_LIB_RESOLVED_API_TOKEN=""
  token="$(dev_token)" || return 7
  EVAL_LIB_RESOLVED_API_TOKEN="$token"
}

# Kept for the benches that call it by name. It resolves; it no longer exports.
dev_env() { dev_resolve; }

dev_auth_header() {
  dev_resolve || return 7
  echo "Authorization: Bearer $EVAL_LIB_RESOLVED_API_TOKEN"
}

# Run ONE child process with the dev connection composed into its environment.
# Those variables live for that process only, which is what keeps a leaked
# parent CODETERM_API_PORT from redirecting the call and leaves this shell
# unchanged. `--as-tab <id>` names the dev tab the child speaks as; the default
# blanks the parent's tab id, which would prefix every send with [from <tab>].
dev_run() {
  local caller_tab_id=""
  if [ "${1:-}" = "--as-tab" ]; then
    caller_tab_id="${2:-}"
    [ -n "$caller_tab_id" ] || {
      echo "ERROR [dev_run]: --as-tab requires a dev Tab id" >&2
      return 7
    }
    shift 2
  fi
  dev_resolve || return 7
  local -a dev_vars=(
    "CODETERM_API_PORT=$EVAL_LIB_DEV_API_PORT"
    "CODETERM_API_TOKEN=$EVAL_LIB_RESOLVED_API_TOKEN"
    "CODETERM_SOCKET_PATH=$EVAL_LIB_RESOLVED_SOCKET_PATH"
    "CODETERM_DEV_DATA_DIR=$EVAL_LIB_RESOLVED_DATA_DIR"
    "CODETERM_DEV_AUTH_HEADER=Authorization: Bearer $EVAL_LIB_RESOLVED_API_TOKEN"
    "CODETERM_TAB_ID=$caller_tab_id"
    "CODETERM_PARENT_TAB_ID="
  )
  # A prod tab's own identity (attempt, nonce, session, team, plan, task) must never reach dev.
  env -u CODETERM_STARTUP_ATTEMPT_ID -u CODETERM_SESSION_BINDING_NONCE -u CODETERM_SESSION_ID     -u CODETERM_TEAM_ID -u CODETERM_PLAN_ID -u CODETERM_TASK_ID -u CODETERM_ATTEMPT_ID     "${dev_vars[@]}" "$@"
}

dev_api_url() {
  printf 'http://%s:%s' "$EVAL_LIB_DEV_HOST" "$EVAL_LIB_DEV_API_PORT"
}

dev_assert_target() {
  local health actual_instance
  if ! health="$(curl -sS --max-time 1 "$(dev_api_url)/api/health")"; then
    dev_resolution_error "${EVAL_LIB_RESOLVED_DATA_DIR:-<unresolved>}" \
      "${EVAL_LIB_RESOLVED_DATA_DIR:-$(dev_default_data_dir)}"
    return 7
  fi
  if [ -n "$EVAL_LIB_RESOLVED_INSTANCE" ]; then
    actual_instance="$(printf '%s\n' "$health" | python3 -c '
import json, sys
print(json.load(sys.stdin).get("instance", {}).get("name", ""))
')"
    if [ "$actual_instance" != "$EVAL_LIB_RESOLVED_INSTANCE" ]; then
      dev_resolution_error "${EVAL_LIB_RESOLVED_DATA_DIR:-<unresolved>}" \
        "${EVAL_LIB_RESOLVED_DATA_DIR:-$(dev_default_data_dir)}"
      return 7
    fi
  fi
}

# Convenience: GET an API path, returns JSON. The selected dev port
# cannot be replaced by an inherited prod CODETERM_API_PORT.
dev_http() {
  local method="$1" path="$2" body="${3:-}" response status auth
  dev_resolve || return 7
  dev_assert_target || return 7
  auth="$(dev_auth_header)" || return 7
  local -a args=(-sS --max-time 30 -o - -w $'\n%{http_code}' -X "$method"
    -H "$auth")
  if [ "$method" = POST ]; then
    args+=(-H "Content-Type: application/json" --data "$body")
  fi
  if ! response="$(curl "${args[@]}" "$(dev_api_url)$path")"; then
    echo "ERROR [dev-http]: $method $path failed before an HTTP response" >&2
    return 7
  fi
  status="${response##*$'\n'}"
  response="${response%$'\n'*}"
  if [ "${CODETERM_HTTP_STATUS_LOG:-0}" = 1 ]; then
    echo "[dev-http] $method $path -> HTTP $status" >&2
  fi
  if [[ "$status" != 2* ]]; then
    echo "ERROR [dev-http]: $method $path -> HTTP $status" >&2
    [ -z "$response" ] || printf '%s\n' "$response" >&2
    return 22
  fi
  printf '%s' "$response"
}

dev_get() { dev_http GET "$1"; }

dev_post() {
  local path="$1"; shift
  # NOTE: "${1:-{}}" is a bash parsing trap — the expansion closes at the FIRST
  # `}`, appending a stray `}` to every non-empty body (invalid JSON).
  local body="${1:-}"
  [ -n "$body" ] || body='{}'
  dev_http POST "$path" "$body"
}

dev_post_as_tab() {
  local tab_id="$1" path="$2" body="${3:-{}}" auth
  [ -n "$tab_id" ] || { echo "dev_post_as_tab requires tab id" >&2; return 7; }
  dev_resolve || return 7
  dev_assert_target || return 7
  auth="$(dev_auth_header)" || return 7
  curl -s --fail --max-time 30 -X POST -H "$auth" \
    -H "X-CodeTerm-Tab-Id: $tab_id" -H "Content-Type: application/json" \
    --data "$body" "$(dev_api_url)$path"
}

# Run the codeterm CLI against the dev daemon, regardless of what
# CODETERM_* vars the parent shell inherited. This is the ONLY way
# bench scripts should invoke the CLI — `codeterm <cmd>` directly
# follows the parent env, which in a prod-hosted Claude tab silently
# routes to port 7685 + the prod socket.
#
# Assert dev is alive BEFORE every call. Otherwise the CLI's socket
# auto-discovery falls back to prod's pipe when codeterm-dev.sock
# doesn't exist — which is exactly how Andrey lost prod state on
# 2026-05-07. Fail loud with non-zero exit instead of silently
# dispatching to the wrong daemon.
dc() {
  local cli caller_tab_id=""
  if [ "${1:-}" = "--as-tab" ]; then
    caller_tab_id="${2:-}"
    [ -n "$caller_tab_id" ] || {
      echo "ERROR [dc]: --as-tab requires a dev Tab id" >&2
      return 7
    }
    shift 2
  fi
  dev_resolve || return 7
  cli="$EVAL_LIB_RESOLVED_DATA_DIR/bin/codeterm"
  [ -x "$cli" ] || [ ! -x "$cli.exe" ] || cli="$cli.exe"
  if [ ! -x "$cli" ]; then
    dev_resolution_error "$cli" "$EVAL_LIB_RESOLVED_DATA_DIR"
    echo "            Build it with: ./build.sh cli" >&2
    return 7
  fi
  if ! dev_assert_target; then
    echo "ERROR [dc]: requested dev daemon is unavailable. Start it: ./build.sh dev" >&2
    echo "             (would have leaked to prod via socket-discovery fallback)" >&2
    return 7
  fi
  if [ -n "$caller_tab_id" ]; then
    # Plan mutations authenticate the live bound session, not just the tab id.
    local inspect session nonce
    inspect="$(dev_run "$cli" tab inspect "$caller_tab_id" --json 2>/dev/null)"
    session="$(printf '%s' "$inspect" | grep -m1 -oE '"agent_session_id": *"[^"]+"' | sed -E 's/.*"([^"]+)"$/\1/')"
    nonce="$(printf '%s' "$inspect" | grep -m1 -oE '"session_binding_nonce": *"[^"]+"' | sed -E 's/.*"([^"]+)"$/\1/')"
    dev_run --as-tab "$caller_tab_id" env \
      "CODETERM_SESSION_ID=$session" "CODETERM_SESSION_BINDING_NONCE=$nonce" "$cli" "$@"
  else
    dev_run "$cli" "$@"
  fi
}

# --- Lifecycle -------------------------------------------------------

# Returns dev PID on stdout if listening on 7686, empty otherwise.
dev_pid() {
  if is_windows; then
    netstat -ano 2>/dev/null | grep ":$EVAL_LIB_DEV_API_PORT.*LISTEN" | awk '{print $5}' | head -1
  elif is_macos || is_linux; then
    lsof -ti:"$EVAL_LIB_DEV_API_PORT" 2>/dev/null | head -1
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
    if curl -s --max-time 1 "$(dev_api_url)/api/health" > /dev/null 2>&1; then
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
  (
    cd "$EVAL_LIB_REPO_ROOT" || exit 1
    local log_dir; log_dir="$(dev_data_dir)/logs" || exit 7
    mkdir -p "$log_dir"
    "${BASH:-bash}" build.sh dev > "$log_dir/codeterm-stdout.log" 2> "$log_dir/codeterm-stderr.log" < /dev/null &
    disown "$!" 2>/dev/null || true
  )
}

# --- Snapshots -------------------------------------------------------

# Capture workspaces + tabs + last.json under target-check/evals/<label>/.
# Echoes the snapshot dir on stdout so callers can chain.
snapshot() {
  local label="${1:?usage: snapshot <label>}"
  local dir="$EVAL_LIB_OUT_BASE/$label"
  mkdir -p "$dir"
  dev_get /api/workspaces > "$dir/ws.json"
  dev_get /api/tabs      > "$dir/tabs.json"
  python3 - "$dir" "$(dev_data_dir)" <<'PYEOF'
import os, sys, shutil
src = os.path.join(sys.argv[2], 'sessions', 'last.json')
if os.path.exists(src):
    shutil.copy(src, os.path.join(sys.argv[1], 'last.json'))
PYEOF
  echo "$dir"
}

# Print PASS/FAIL diff of two snapshot dirs. Exit 0 if all match, 1 if any drift.
# Compares: workspace ids/names/cwds/tab_counts AND tab ids/titles/agent_types/cwds.
diff_snapshots() {
  local pre="${1:?usage: diff_snapshots <pre_dir> <post_dir>}"
  local post="${2:?}"
  PYTHONIOENCODING=utf-8 python3 - "$pre" "$post" <<'PYEOF'
import json, sys, os
pre, post = sys.argv[1], sys.argv[2]
pre_ws = json.load(open(os.path.join(pre, 'ws.json')))
post_ws = json.load(open(os.path.join(post, 'ws.json')))
pre_p = {p['id']: p for p in json.load(open(os.path.join(pre, 'tabs.json')))}
post_p = {p['id']: p for p in json.load(open(os.path.join(post, 'tabs.json')))}

print(f'  workspaces: pre={len(pre_ws)} post={len(post_ws)}')
print(f'  tabs:      pre={len(pre_p)} post={len(post_p)}')

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
        print(f'  FAIL tab LOST: {pid} title={a.get("title")!r} agent={a.get("agent_type")}')
        fail += 1
        continue
    diffs = []
    for k in ('title','agent_type','cwd'):
        if a.get(k) != b.get(k): diffs.append(f'{k}: {a.get(k)!r}->{b.get(k)!r}')
    if diffs:
        print(f'  FAIL tab {pid}: ' + '; '.join(diffs))
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
  dev_get /api/workspaces | python3 -c "
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
    ids=$(dev_get /api/workspaces | python3 -c "
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
  local data_dir; data_dir="$(dev_data_dir)" || return 7
  rm -f "$data_dir/sessions/last.json"
  rm -rf "$data_dir/sessions/backups"
  dev_start_bg
  dev_wait_ready 600
  sleep 3
}

# Sanity check the on-disk session file is well-formed and that no
# half-written .tmp got left behind. Returns non-zero on either issue.
# Uses python3's os.path.expanduser so the path resolves correctly on
# Windows (git bash $HOME is /c/Users/X — invalid for native Python).
assert_atomic_save_clean() {
  python3 - "$(dev_data_dir)" <<'PYEOF' || { echo "  FAIL session file integrity check"; return 1; }
import json, os, sys
sess = os.path.join(sys.argv[1], 'sessions')
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

# --- Plan-junk cleanup (test-harness hygiene) ------------------------
# Matrix/creation/restore benches spin up throwaway plans; every one MUST
# be deleted, even on failure/abort. Pattern: snapshot the plan-id set at
# bench start, sweep everything created since at cleanup (by id-diff, NOT
# by title — a regressed plan named "Orchestration"/"04:56" is still
# swept), then assert before == after so a leak FAILS the bench.

# NOTE: `python3 -c` (script via argv) — NOT `python3 - <<HEREDOC`. With a pipe
# the heredoc becomes stdin, so json.load(sys.stdin) reads empty and the sweep
# silently no-ops (false-green). Single-quote the -c body so $/\ stay literal.
plan_ids() {
  dc plan list --json 2>/dev/null | python3 -c '
import json,sys
try: d=json.load(sys.stdin)
except Exception: d=[]
for p in (d if isinstance(d,list) else d.get("plans",[])):
    pid=p.get("plan_id") or p.get("id") or ""
    if pid: print(pid)
' 2>/dev/null
}

# Snapshot current plan ids (sorted) to $1. Requires dev alive — an empty
# baseline from a dead daemon would make plan_sweep_new delete everything.
plan_snapshot() {
  local out="${1:?usage: plan_snapshot <file>}"
  dev_alive || { echo "plan_snapshot: dev not on 7686 — refusing (unsafe empty baseline)" >&2; return 1; }
  plan_ids | sort > "$out"
}

# Delete one plan id with retries (delete no-ops under load: db lock / exit 7).
plan_delete_one() {
  local id="$1" t
  [ -n "$id" ] || return 0
  for t in 1 2 3 4 5; do
    dc plan delete "$id" 2>/dev/null | grep -qi "OK" && return 0
    sleep 1
  done
  return 1
}

# Delete every plan absent from the baseline snapshot ($1). Idempotent —
# safe from a trap on abort AND again on the happy path.
plan_sweep_new() {
  local base="${1:?usage: plan_sweep_new <baseline_file>}" id
  [ -f "$base" ] || return 0
  comm -13 "$base" <(plan_ids | sort) 2>/dev/null | while read -r id; do
    [ -n "$id" ] || continue
    if plan_delete_one "$id"; then echo "    · swept plan $id" >&2
    else echo "    · WARN could not delete plan $id (manual: dc plan delete $id)" >&2; fi
  done
}

# Emit "<id>\t<title>" for every plan whose title marks matrix/creation
# junk: empty, bare 'Orchestration', 'orchmatrix*', or a HH:MM name.
plan_junk_titles() {
  dc plan list --json 2>/dev/null | python3 -c '
import json,re,sys
try: d=json.load(sys.stdin)
except Exception: d=[]
pat=re.compile(r"^(orchestration|\d{1,2}:\d{2})$", re.I)
for p in (d if isinstance(d,list) else d.get("plans",[])):
    t=(p.get("title") or "").strip()
    if t=="" or pat.match(t) or "orchmatrix" in t.lower():
        print("%s\t%r" % (p.get("plan_id") or p.get("id") or "?", t))
' 2>/dev/null
}

# Assert the plan set returned to baseline ($1) AND no junk-titled plans
# remain. $2 = label. Prints + returns non-zero on any leak.
plan_assert_clean() {
  local base="${1:?}" label="${2:-run}" leaked junk rc=0
  leaked="$(comm -13 "$base" <(plan_ids | sort) 2>/dev/null)"
  if [ -n "$leaked" ]; then
    echo "  FAIL [$label] plan leak (before != after):" >&2
    printf '%s\n' "$leaked" | sed 's/^/    + /' >&2
    rc=1
  fi
  junk="$(plan_junk_titles)"
  if [ -n "$junk" ]; then
    echo "  FAIL [$label] junk-titled plans present:" >&2
    printf '%s\n' "$junk" | sed 's/^/    ! /' >&2
    rc=1
  fi
  [ "$rc" = 0 ] && echo "  OK [$label] plan set clean (before == after, no junk)"
  return $rc
}

# --- One-shot scenario builders --------------------------------------

# Create a workspace + return its id on stdout. Args: name, cwd.
new_workspace() {
  local name="$1" cwd="$2"
  mkdir -p "$cwd" 2>/dev/null
  dc workspace create "$name" --cwd "$cwd" --json 2>/dev/null \
    | python3 -c "import json,sys; print(json.load(sys.stdin).get('id',''))"
}

# Workspace whose directory the daemon creates, so agents started in it get directory trust.
# Echoes "<id><TAB><cwd>"; the caller never creates or names the directory.
new_provisioned_workspace() {
  local name="$1"
  dc workspace create "$name" --provision --json 2>/dev/null \
    | python3 -c "import json,sys; w=json.load(sys.stdin); print(w.get('id','') + '\t' + w.get('cwd',''))"
}

# Spawn a plain shell tab in the given workspace. Echoes tab_id.
new_shell_tab() {
  local ws="$1" cwd="$2" title="${3:-shell}"
  dc tab new --workspace "$ws" --cwd "$cwd" --title "$title" --json 2>/dev/null \
    | python3 -c "import json,sys; print(json.load(sys.stdin).get('tab_id',''))"
}

# Spawn an agent tab of the given provider. Echoes tab_id.
# The task goes through `--task-file -` so a bench prompt with backticks, $,
# quotes or newlines reaches the agent byte-exact; an inline --task of that
# shape is refused by the CLI, because the shell would already have eaten it.
new_agent_tab() {
  local provider="$1" ws="$2" cwd="$3" title="$4" task="${5:-}"
  local args=( agent spawn "$provider" --workspace "$ws" --cwd "$cwd" --title "$title" )
  if [ -n "$task" ]; then
    printf '%s' "$task" | dc "${args[@]}" --task-file - 2>&1 \
      | grep -oE "tab [a-f0-9]+" | awk '{print $2}' | head -1
  else
    dc "${args[@]}" 2>&1 | grep -oE "tab [a-f0-9]+" | awk '{print $2}' | head -1
  fi
}

# Spawn a claude tab in the given workspace. Echoes tab_id.
# Optional 4th arg = initial task prompt.
new_claude_tab() {
  new_agent_tab claude "$1" "$2" "${3:-claude}" "${4:-}"
}

# Spawn a codex tab. Echoes tab_id. Optional 4th arg = task prompt.
new_codex_tab() {
  new_agent_tab codex "$1" "$2" "${3:-codex}" "${4:-}"
}

# Spawn an opencode tab. Echoes tab_id. Optional 4th arg = task prompt.
new_opencode_tab() {
  new_agent_tab opencode "$1" "$2" "${3:-opencode}" "${4:-}"
}

# --- Auto-init: resolve the dev connection at source time. -----------
# Resolution only, into lib-private state. Every helper composes the
# connection into the process it invokes, so a bench that forgets to resolve
# still cannot reach prod, and the shell that sourced this file keeps the
# CODETERM_* it already had. Andrey's prod state got minor cleanup damage
# from an inherited prod port on 2026-05-07; composition is what prevents it.
dev_resolve > /dev/null 2>&1 || true
