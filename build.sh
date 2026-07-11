#!/usr/bin/env bash
# CodeTerm build script — works on macOS, Linux, and Windows.
# Usage:
#   ./build.sh dev       — dev mode (hot reload, incremental)
#   ./build.sh dev --wait — same, but backgrounds and blocks until /api/health
#                          responds (handy for scripts/agents)
#   ./build.sh check     — integration-owner cargo check
#   ./build.sh worktree-check — explicit isolated, nonincremental lane check
#   ./build.sh build     — production build
#   ./build.sh bootstrap — create/build bundled CLI + embed sidecar for fresh clones
#   ./build.sh qa-prep   — one-shot: dist + sidecars for a fresh isolated worktree (no tsc gate)
#   ./build.sh cli       — build CLI and install to ~/.codeterm/bin/ + ~/.codeterm-dev/bin/
#   ./build.sh clean     — clean build cache
#   ./build.sh kill      — kill dev instance (safe, won't touch prod)
#   ./build.sh e2e       — run non-LLM E2E tests against dev (port 7686)
#   ./build.sh e2e:llm   — run LLM E2E tests against dev (real provider tokens)
#   source build.sh env  — load MSVC env vars into current shell (Windows only)

set -e

CMD="${1:-dev}"

# --- Load .env (build secrets like UPDATER_TOKEN) ---
if [ -f ".env" ]; then
  set -a
  source .env
  set +a
fi

# --- MSVC environment (Windows only) ---
setup_env() {
  if [[ "$OSTYPE" != darwin* ]] && [[ "$OSTYPE" != linux* ]]; then
    BT_BASE="C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools"
    SDK_BASE="C:/Program Files (x86)/Windows Kits/10"
    MSVC_VER=$(ls -1 "$BT_BASE/VC/Tools/MSVC" 2>/dev/null | sort -V | tail -1)
    SDK_VER=$(ls -1 "$SDK_BASE/Include" 2>/dev/null | grep -E '^10\.' | sort -V | tail -1)
    if [ -z "$MSVC_VER" ] || [ -z "$SDK_VER" ]; then
      echo "ERROR: MSVC ($BT_BASE) or Windows SDK ($SDK_BASE) not found" >&2
      return 1
    fi
    export CC="$BT_BASE/VC/Tools/MSVC/$MSVC_VER/bin/Hostx64/x64/cl.exe"
    export CXX="$BT_BASE/VC/Tools/MSVC/$MSVC_VER/bin/Hostx64/x64/cl.exe"
    export LIB="$BT_BASE/VC/Tools/MSVC/$MSVC_VER/lib/x64;$SDK_BASE/Lib/$SDK_VER/ucrt/x64;$SDK_BASE/Lib/$SDK_VER/um/x64"
    export INCLUDE="$BT_BASE/VC/Tools/MSVC/$MSVC_VER/include;$SDK_BASE/Include/$SDK_VER/ucrt;$SDK_BASE/Include/$SDK_VER/um;$SDK_BASE/Include/$SDK_VER/shared"
    export RC="$SDK_BASE/bin/$SDK_VER/x64/rc.exe"
    # Use /c/ prefix for git bash PATH compatibility
    export PATH="/c/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/MSVC/$MSVC_VER/bin/HostX64/x64:/c/Program Files (x86)/Windows Kits/10/bin/$SDK_VER/x64:$PATH"
    echo "MSVC env loaded (VS 2022 BuildTools, MSVC $MSVC_VER)"
  fi
}

# --- Kill THIS dev instance only (never prod, never another worktree) ---
# Scoped strictly to $CT_PORT / $CT_VITE_PORT / this instance's screen+pidfile.
# No global pgrep on target/debug/codeterm — that would take down every worktree.
kill_dev() {
  if [ "$CT_PORT" = "7685" ]; then
    echo "ERROR: refusing to kill prod CodeTerm port 7685" >&2
    exit 1
  fi
  if [ "${CODETERM_TEST_KILL_DRY_RUN:-0}" = "1" ]; then
    echo "Dry-run dev kill instance='${CT_INSTANCE:-default}' api=$CT_PORT vite=$CT_VITE_PORT cdp=$CT_CDP_PORT identifier=$CT_IDENTIFIER"
    return
  fi
  kill_web_watch 2>/dev/null || true
  local session_prefix="codeterm-dev${CT_SUFFIX}-"
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    if command -v screen >/dev/null 2>&1; then
      screen -ls 2>/dev/null | awk -v p="$session_prefix" 'index($1,p){print $1}' | while read -r session; do
        screen -S "$session" -X quit 2>/dev/null || true
      done
    fi
    local dev_pids
    dev_pids="$(lsof -tiTCP:"$CT_PORT" -sTCP:LISTEN 2>/dev/null || true)"
    if [ -n "$dev_pids" ]; then
      echo "$dev_pids" | xargs kill 2>/dev/null || true
      for _ in $(seq 1 30); do
        [ -z "$(lsof -tiTCP:"$CT_PORT" -sTCP:LISTEN 2>/dev/null || true)" ] && break
        sleep 0.1
      done
      dev_pids="$(lsof -tiTCP:"$CT_PORT" -sTCP:LISTEN 2>/dev/null || true)"
      [ -z "$dev_pids" ] || echo "$dev_pids" | xargs kill -9 2>/dev/null || true
    fi
    local vite_pids
    vite_pids="$(lsof -ti:"$CT_VITE_PORT" 2>/dev/null || true)"
    [ -z "$vite_pids" ] || echo "$vite_pids" | xargs kill -9 2>/dev/null || true
    if [ -f "$CT_DATA_DIR/daemon.pid" ]; then
      kill "$(cat "$CT_DATA_DIR/daemon.pid")" 2>/dev/null || true
      rm -f "$CT_DATA_DIR/daemon.pid"
    fi
  else
    local dev_pid
    dev_pid=$(netstat -ano 2>/dev/null | grep ":$CT_PORT.*LISTEN" | awk '{print $5}' | head -1 || true)
    if [ -n "$dev_pid" ] && [ "$dev_pid" != "0" ]; then
      echo "Killing dev instance (PID $dev_pid on port $CT_PORT)..."
      taskkill //PID "$dev_pid" //T //F 2>/dev/null || true
      sleep 1
    fi
    local vite_pid
    vite_pid=$(netstat -ano 2>/dev/null | grep ":$CT_VITE_PORT.*LISTEN" | awk '{print $5}' | head -1 || true)
    if [ -n "$vite_pid" ] && [ "$vite_pid" != "0" ]; then
      taskkill //PID "$vite_pid" //T //F 2>/dev/null || true
    fi
    windows_dev_leftover_cleanup
  fi
}

CMD="${1:-dev}"
case "$CMD" in
  gateway-*) ;;
  *) setup_env ;;
esac

# --- Per-worktree dev instance isolation ---
# CODETERM_INSTANCE (optional) names a dev instance so multiple worktrees can
# run ./build.sh dev concurrently without colliding. Unset = canonical defaults
# (.codeterm-dev / 7686 / 1420). See AGENTS.md "Process safety".
CT_INSTANCE="${CODETERM_INSTANCE:-}"
CT_PORT="${CODETERM_API_PORT:-7686}"
case "$CMD" in
  dev|kill|e2e|e2e:llm)
    if [ "$CT_PORT" = "7685" ]; then
      if [ -n "$CT_INSTANCE" ]; then
        echo "ERROR: refusing to run $CMD against prod port 7685 (CODETERM_INSTANCE=$CT_INSTANCE)." >&2
        echo "Pick a non-prod CODETERM_API_PORT for this dev instance." >&2
        exit 2
      fi
      echo "WARNING: ignoring inherited CODETERM_API_PORT=7685 for $CMD; using dev port 7686." >&2
      CT_PORT=7686
    fi
    ;;
esac
if [ -n "$CT_INSTANCE" ]; then
  CT_SLUG=$(printf '%s' "$CT_INSTANCE" | tr '[:upper:]' '[:lower:]' | sed 's/[^a-z0-9-]/-/g')
  CT_SUFFIX="-$CT_SLUG"
  CT_IDENTIFIER="com.codeterm.dev.$(printf '%s' "$CT_SLUG" | tr '-' '.')"
  # Stable per-instance vite port: sum of slug bytes mod 200, +1 offset.
  CT_OFFSET=$(printf '%s' "$CT_SLUG" | od -An -tu1 | awk '{for(i=1;i<=NF;i++)s+=$i} END{print (s%200)+1}')
  CT_VITE_PORT=$((1420 + CT_OFFSET))
  CT_CDP_PORT=$((9223 + CT_OFFSET))
else
  CT_SLUG=""
  CT_SUFFIX=""
  CT_IDENTIFIER="com.codeterm.dev"
  CT_VITE_PORT=1420
  CT_CDP_PORT=9223
fi
CT_DATA_DIR="$HOME/.codeterm-dev${CT_SUFFIX}"
# Export so the spawned daemon + its child PTYs inherit the instance identity.
export CODETERM_INSTANCE="$CT_INSTANCE"
export CODETERM_API_PORT="$CT_PORT"
export CODETERM_VITE_PORT="$CT_VITE_PORT"
export CODETERM_DEV_IDENTIFIER="$CT_IDENTIFIER"
export CODETERM_CDP_PORT="$CT_CDP_PORT"

# Wire .githooks as the hooks directory
git config core.hooksPath .githooks 2>/dev/null || true

source scripts/cache-policy.sh
source scripts/build-gateway.sh

if [ -n "${CODETERM_BUILD_GATEWAY_DIR:-}" ]; then
  echo "ERROR: CODETERM_BUILD_GATEWAY_DIR cannot redirect the machine build gateway" >&2
  exit 2
fi
if [ -n "${CT_BUILD_GATEWAY_SCOPE:-}" ]; then
  echo "ERROR: CT_BUILD_GATEWAY_SCOPE is internal to fake gateway commands" >&2
  exit 2
fi
case "$CMD" in
  gateway-fake|gateway-sleep|gateway-status|gateway-cancel|gateway-policy-probe)
    [ -z "${CODETERM_BUILD_GATEWAY_FIXTURE_DIR:-}" ] || ct_gateway_set_fixture_dir "$CODETERM_BUILD_GATEWAY_FIXTURE_DIR"
    ;;
  *)
    if [ -n "${CODETERM_BUILD_GATEWAY_FIXTURE_DIR:-}" ]; then
      echo "ERROR: fixture gateway scope is unavailable to production orchestration command '$CMD'" >&2
      exit 2
    fi
    ;;
esac

enter_build_gateway() {
  ct_gateway_acquire "$*"
  local gateway_log
  gateway_log="$(ct_gateway_log_path)"
  echo "Build gateway job $CT_BUILD_JOB_ID log: $gateway_log"
  exec > >(tee -a "$gateway_log") 2>&1
}

integration_worktree() {
  local canonical
  if [ -n "${CODETERM_INTEGRATION_WORKTREE:-}" ]; then
    echo "ERROR: CODETERM_INTEGRATION_WORKTREE cannot designate build ownership" >&2
    return 2
  fi
  canonical="$(git worktree list --porcelain 2>/dev/null | awk '
    /^worktree / { path=substr($0,10) }
    /^branch refs\/heads\/feat\/1[.]7[.]2-stability$/ { print path }
  ')"
  if [ -n "$canonical" ]; then
    printf '%s\n' "$canonical"
    return
  fi
  git worktree list --porcelain 2>/dev/null | awk '
    /^worktree / { path=substr($0,10) }
    /^branch refs\/heads\/release\/.*-integration$/ { print path }
  '
}

require_integration_owner() {
  local integration current current_branch count target_abs
  integration="$(integration_worktree)" || exit $?
  current="$(pwd -P)"
  count="$(printf '%s\n' "$integration" | sed '/^$/d' | wc -l | tr -d ' ')"
  if [ "$count" -gt 1 ]; then
    echo "ERROR: multiple registered release/*-integration worktrees; build ownership is ambiguous." >&2
    printf '%s\n' "$integration" >&2
    exit 2
  fi
  if [ -z "$integration" ]; then
    echo "ERROR: no release/*-integration worktree is registered for $CMD." >&2
    exit 2
  fi
  integration="$(cd "$integration" && pwd -P)"
  current_branch="$(git symbolic-ref --quiet --short HEAD 2>/dev/null || true)"
  if [ "$integration" != "$current" ] || { [[ "$current_branch" != "feat/1.7.2-stability" ]] && [[ ! "$current_branch" =~ ^release/.+-integration$ ]]; }; then
    echo "ERROR: $CMD is restricted to the canonical stability/integration checkout." >&2
    echo "Current worktree: $current" >&2
    echo "Integration checkout: $integration" >&2
    echo "Worker worktrees should submit diffs; the gateway builds once from integration." >&2
    exit 2
  fi
  if [ -n "${CARGO_TARGET_DIR:-}" ]; then
    target_abs="$(node -e 'const path=require("node:path");process.stdout.write(path.resolve(process.argv[1]))' "$CARGO_TARGET_DIR")"
    if ! ct_path_under "$target_abs" "$current"; then
      echo "ERROR: integration mutable target must live inside the registered warm checkout: $target_abs" >&2
      exit 2
    fi
  fi
  unset CODETERM_ALLOW_EXTERNAL_TARGET
}

prepare_exceptional_worktree_target() {
  local dir identity
  dir="$(ct_gateway_dir)"
  identity="$(printf '%s' "$(pwd -P)" | git hash-object --stdin)"
  export CARGO_TARGET_DIR="$dir/targets/exceptional/$identity"
  export CARGO_INCREMENTAL=0
  export CODETERM_ALLOW_EXTERNAL_TARGET=1
  mkdir -p "$CARGO_TARGET_DIR"
  ct_verify_target_dir
  ct_require_prestarted_sccache
  echo "[cache-policy] exceptional worktree target=$CARGO_TARGET_DIR"
  echo "[cache-policy] CARGO_INCREMENTAL=$CARGO_INCREMENTAL"
}

prepare_release_target() {
  local dir
  dir="$(ct_gateway_dir)/targets/release/$CT_BUILD_JOB_ID"
  if [ -e "$dir" ]; then
    echo "ERROR: release target is not fresh: $dir" >&2
    exit 2
  fi
  export CARGO_TARGET_DIR="$dir"
  export CARGO_INCREMENTAL=0
  export CODETERM_ALLOW_EXTERNAL_TARGET=1
  mkdir -p "$CARGO_TARGET_DIR"
  echo "[cache-policy] fresh release target=$CARGO_TARGET_DIR"
  echo "[cache-policy] CARGO_INCREMENTAL=$CARGO_INCREMENTAL"
}

require_release_source() {
  local actual expected dirty
  actual="$(git rev-parse HEAD)"
  expected="${CODETERM_EXPECTED_BUILD_SHA:-$actual}"
  if [ "$actual" != "$expected" ]; then
    echo "ERROR: release source mismatch: expected $expected, got $actual" >&2
    exit 2
  fi
  dirty="$(git status --porcelain=v1)"
  if [ -n "$dirty" ]; then
    echo "ERROR: release source is not clean; refusing provenance capture" >&2
    echo "$dirty" >&2
    exit 2
  fi
}

# Copy the freshly-built CLI into a target bin dir. Both prod (~/.codeterm/bin)
# and dev (~/.codeterm-dev/bin) need the same binary so the dev daemon's
# resolve_cli_path (which prefers data_dir/bin first) doesn't shell into a
# stale CLI that's missing recent subcommands.
cli_release_dir() {
  local target_root="${CARGO_TARGET_DIR:-cli/target}"
  if [ -n "${CARGO_BUILD_TARGET:-}" ]; then
    echo "$target_root/$CARGO_BUILD_TARGET/release"
  else
    echo "$target_root/release"
  fi
}

tauri_debug_dir() {
  local target_root="${CARGO_TARGET_DIR:-src-tauri/target}"
  if [ -n "${CARGO_BUILD_TARGET:-}" ]; then
    echo "$target_root/$CARGO_BUILD_TARGET/debug"
  else
    echo "$target_root/debug"
  fi
}

install_cli_to() {
  local bin_dir="$1"
  local release_dir
  release_dir="$(cli_release_dir)"
  mkdir -p "$bin_dir"
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    cp "$release_dir/codeterm" "$bin_dir/codeterm"
  else
    cp "$release_dir/codeterm.exe" "$bin_dir/codeterm.exe"
  fi
  echo "CLI installed to $bin_dir"
}

if [ "${CODETERM_TEST_PRINT_CLI_RELEASE_DIR:-}" = "1" ]; then
  cli_release_dir
  exit 0
fi

if [ "${CODETERM_TEST_PRINT_BUILD_CACHE_POLICY:-}" = "1" ]; then
  ct_verify_target_dir
  printf 'identifier=%s\napi_port=%s\nvite_port=%s\ncdp_port=%s\ntauri_debug_dir=%s\n' \
    "$CT_IDENTIFIER" "$CT_PORT" "$CT_VITE_PORT" "$CT_CDP_PORT" "$(tauri_debug_dir)"
  exit 0
fi

prepare_bundle_stubs() {
  node scripts/prepare-cli-stub.js
  node scripts/prepare-embed-stub.js
}

prepare_dev_frontend_dist() {
  node scripts/dev-resource-preflight.mjs "${1:-$(pwd -P)}"
}

# Resolve the tauri dev config. For a named instance, generate a derived config
# whose devUrl points at this instance's vite port (vite reads CODETERM_VITE_PORT,
# but tauri.dev.conf.json's devUrl can't be templated, so we materialize it).
dev_conf_path() {
  local base="src-tauri/tauri.dev.conf.json"
  if [ -z "$CT_SUFFIX" ]; then
    echo "$base"
    return
  fi
  local out="src-tauri/tauri.dev.conf${CT_SUFFIX}.json"
  node -e "const fs=require('fs');const c=JSON.parse(fs.readFileSync('$base','utf8'));c.identifier=process.env.CODETERM_DEV_IDENTIFIER;c.build=c.build||{};c.build.devUrl='http://127.0.0.1:'+process.env.CODETERM_VITE_PORT;fs.writeFileSync('$out',JSON.stringify(c,null,2));" >&2
  echo "$out"
}

ensure_real_embed_bundle() {
  local host exe_ext embed_bundle embed_size
  host=$(rustc -vV | awk -F': ' '/^host:/{print $2}')
  exe_ext=""
  case "$OSTYPE" in msys*|cygwin*|win32) exe_ext=".exe" ;; esac
  embed_bundle="src-tauri/bundled-embed/codeterm-embed-${host}${exe_ext}"
  prepare_bundle_stubs
  embed_size=0
  if [ -f "$embed_bundle" ]; then
    embed_size=$(stat -c %s "$embed_bundle" 2>/dev/null || stat -f %z "$embed_bundle" 2>/dev/null || echo 0)
  fi
  if [ "${REBUILD_EMBED:-0}" = "1" ] || [ "$embed_size" -lt 1048576 ]; then
    echo "Building codeterm-embed before dev (size=${embed_size}, REBUILD_EMBED=${REBUILD_EMBED:-0})..."
    cargo build --release --manifest-path src-tauri/Cargo.toml --bin codeterm-embed
    node scripts/bundle-embed.js
  fi
}

start_dev_wait_process() {
  local log="$1"
  local conf
  conf="$(dev_conf_path)"
  local cmd=(pnpm tauri dev --no-watch --config "$conf")
  if [[ "$OSTYPE" == darwin* ]] && command -v screen >/dev/null 2>&1; then
    local session="codeterm-dev${CT_SUFFIX}-$$"
    local root_q log_q conf_q
    printf -v root_q "%q" "$(pwd)"
    printf -v log_q "%q" "$log"
    printf -v conf_q "%q" "$conf"
    screen -dmS "$session" bash -lc "cd $root_q && exec pnpm tauri dev --no-watch --config $conf_q >> $log_q 2>&1"
    echo "screen:$session"
    return
  fi
  nohup "${cmd[@]}" </dev/null >> "$log" 2>&1 &
  echo $!
}

dev_wait_process_alive() {
  local pid="$1"
  case "$pid" in
    screen:*)
      local session="${pid#screen:}"
      screen -ls 2>/dev/null | grep -q "[.]$session[[:space:]]"
      ;;
    *)
      kill -0 "$pid" 2>/dev/null
      ;;
  esac
}

handoff_gateway_to_dev() {
  local dev_pid="$1" monitor_pid monitor_start lock
  lock="$(ct_gateway_dir)/lock"
  (
    trap - EXIT INT TERM
    CT_GATEWAY_MUTEX_PID="$BASHPID"
    CT_GATEWAY_PROCESS_START="$(ct_gateway_process_start_token "$BASHPID")"
    sleep 1
    while dev_wait_process_alive "$dev_pid"; do
      sleep 2
    done
    CT_GATEWAY_ACTIVE=1
    ct_gateway_finish 0
  ) >/dev/null 2>&1 &
  monitor_pid=$!
  monitor_start="$(ct_gateway_process_start_token "$monitor_pid")"
  printf '%s\n' "$monitor_pid" > "$lock/pid"
  printf '%s\n' "$monitor_start" > "$lock/start_token"
  CT_GATEWAY_ACTIVE=0
  echo "Build gateway remains owned by dev monitor $monitor_pid until launcher $dev_pid exits."
}

# /web browser clients are served the static dist/ bundle, NOT vite HMR.
# Watch-rebuild it so phone/browser mirrors pick up frontend edits on reload.
# Tauri's beforeDevCommand owns the live devUrl server on 1420.
web_watch_pidfile() {
  echo "/tmp/codeterm-web-watch${CT_SUFFIX}.pid"
}

kill_process_tree() {
  local pid="$1"
  if [ -z "$pid" ] || ! kill -0 "$pid" 2>/dev/null; then
    return
  fi
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    local child
    for child in $(pgrep -P "$pid" 2>/dev/null || true); do
      kill_process_tree "$child"
    done
  else
    taskkill //PID "$pid" //T //F 2>/dev/null || true
    return
  fi
  kill "$pid" 2>/dev/null || true
}

kill_orphan_web_watchers() {
  if [[ "$OSTYPE" != darwin* ]] && [[ "$OSTYPE" != linux* ]]; then
    windows_dev_leftover_cleanup
    return
  fi
  local root pid cwd
  root="$(pwd)"
  for pid in $(pgrep -f "vite build --watch" 2>/dev/null || true); do
    cwd=$(lsof -a -p "$pid" -d cwd -Fn 2>/dev/null | sed -n 's/^n//p')
    if [ "$cwd" = "$root" ]; then
      kill_process_tree "$pid"
    fi
  done
}

windows_dev_leftover_cleanup() {
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    return
  fi
  command -v powershell.exe >/dev/null 2>&1 || command -v powershell >/dev/null 2>&1 || return

  local root_win target_debug_win cleanup_script ps
  root_win="$(pwd -W 2>/dev/null || pwd)"
  target_debug_win="$(cd "$(tauri_debug_dir)" 2>/dev/null && pwd -W 2>/dev/null || echo "$(pwd -W 2>/dev/null || pwd)/$(tauri_debug_dir)")"
  cleanup_script="$root_win/scripts/windows-dev-cleanup.ps1"
  if command -v powershell.exe >/dev/null 2>&1; then
    ps="powershell.exe"
  else
    ps="powershell"
  fi
  "$ps" -NoProfile -ExecutionPolicy Bypass -File "$cleanup_script" -Root "$root_win" \
    -TargetDebug "$target_debug_win" -Identifier "$CT_IDENTIFIER" -Instance "$CT_INSTANCE" \
    -VitePort "$CT_VITE_PORT" -ApiPort "$CT_PORT" -CdpPort "$CT_CDP_PORT" 2>/dev/null || true
}

start_web_watch() {
  kill_web_watch
  local log="/tmp/codeterm-web-watch${CT_SUFFIX}.log"
  nohup pnpm exec vite build --watch </dev/null >> "$log" 2>&1 &
  echo $! > "$(web_watch_pidfile)"
  echo "Web bundle watcher started (dist/ auto-rebuilds, log $log)"
}

kill_web_watch() {
  local pidfile
  pidfile="$(web_watch_pidfile)"
  if [ -f "$pidfile" ]; then
    kill_process_tree "$(cat "$pidfile")"
    rm -f "$pidfile"
  fi
  kill_orphan_web_watchers
}

dev_health_status() {
  curl -s --max-time 2 "http://127.0.0.1:${CT_PORT}/api/health" 2>/dev/null || true
}

dev_health_fields() {
  node -e '
    let input="";
    process.stdin.on("data", chunk => input += chunk);
    process.stdin.on("end", () => {
      const json=JSON.parse(input);
      const instance=json.instance ?? {};
      process.stdout.write([json.status ?? "", json.build_sha ?? "", instance.name ?? "", instance.api_port ?? "", instance.vite_port ?? "", instance.cdp_port ?? "", instance.identifier ?? ""].join("\n"));
    });
  '
}

validate_dev_health() {
  local expected_sha="$1" expected_instance="$2" expected_port="$3" expected_vite="$4" expected_cdp="$5" expected_identifier="$6" health="$7"
  local status sha instance port vite cdp identifier
  local -a fields=()
  mapfile -t fields < <(printf '%s' "$health" | dev_health_fields 2>/dev/null || true)
  status="${fields[0]:-}"
  sha="${fields[1]:-}"
  instance="${fields[2]:-}"
  port="${fields[3]:-}"
  vite="${fields[4]:-}"
  cdp="${fields[5]:-}"
  identifier="${fields[6]:-}"
  if [ "$status" != "ok" ]; then
    echo "Health endpoint did not report status=ok" >&2
    return 1
  fi
  if [[ ! "$expected_sha" =~ ^[0-9a-f]{40}$ ]] || [[ ! "$sha" =~ ^[0-9a-f]{40}$ ]] || [ "$sha" != "$expected_sha" ]; then
    echo "Health endpoint is stale or wrong-lane: expected build $expected_sha, got $sha" >&2
    return 1
  fi
  if [ "$instance" != "$expected_instance" ] || [ "$port" != "$expected_port" ] ||
      [ "$vite" != "$expected_vite" ] || [ "$cdp" != "$expected_cdp" ] || [ "$identifier" != "$expected_identifier" ]; then
    echo "Health endpoint is from the wrong instance: expected instance='$expected_instance' api='$expected_port' vite='$expected_vite' cdp='$expected_cdp' identifier='$expected_identifier', got instance='$instance' api='$port' vite='$vite' cdp='$cdp' identifier='$identifier'" >&2
    return 1
  fi
  printf '%s\n' "$sha"
}

dev_listener_pid() {
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    lsof -tiTCP:"$CT_PORT" -sTCP:LISTEN 2>/dev/null | head -1 || true
  else
    netstat -ano 2>/dev/null | grep ":$CT_PORT.*LISTEN" | awk '{print $5}' | head -1 || true
  fi
}

# Auto-install node deps if lockfile is newer than node_modules
ensure_deps() {
  if [ ! -d node_modules ] || [ pnpm-lock.yaml -nt node_modules/.package-lock.json ] 2>/dev/null; then
    echo "Installing dependencies..."
    # In no-TTY shells (agents, CI) pnpm aborts when it needs to remove node_modules
    # without a terminal to confirm. CI=true disables that interactive guard.
    [ -t 0 ] || export CI=true
    pnpm install --frozen-lockfile 2>/dev/null || pnpm install
  fi
}

case "$CMD" in
  env)
    # Already loaded above. Use: source build.sh env
    ;;
  fmt-check)
    cargo fmt --manifest-path src-tauri/Cargo.toml --check
    cargo fmt --manifest-path cli/Cargo.toml --check
    ;;
  gateway-status)
    ct_gateway_status
    ;;
  gateway-cancel)
    ct_gateway_cancel "${2:?gateway-cancel requires a job id}"
    ;;
  gateway-health-check)
    validate_dev_health "${2:?expected SHA required}" "${3:-}" "${4:?expected port required}" \
      "${5:?expected vite port required}" "${6:?expected cdp port required}" "${7:?expected identifier required}" \
      "$(cat "${8:?health fixture path required}")"
    ;;
  gateway-fake|gateway-sleep)
    enter_build_gateway "$CMD ${*:2}"
    fake_name="${2:-fake}"
    fake_duration="${3:-${2:-1}}"
    if [ -n "${CODETERM_GATEWAY_FAKE_EVENTS:-}" ]; then
      printf '%s start %s\n' "$fake_name" "$(node -p '(BigInt(Date.now()) * 1000000n).toString()')" >> "$CODETERM_GATEWAY_FAKE_EVENTS"
    fi
    if [[ "$fake_duration" == wait:* ]]; then
      fake_release="${fake_duration#wait:}"
      fake_deadline=$(( $(date +%s) + ${CODETERM_GATEWAY_FAKE_WAIT_SECS:-30} ))
      while [ ! -e "$fake_release" ]; do
        [ "$(date +%s)" -le "$fake_deadline" ] || { echo "Fake gateway release timed out: $fake_release" >&2; exit 124; }
        sleep 0.05
      done
    else
      sleep "$fake_duration"
    fi
    if [ -n "${CODETERM_GATEWAY_FAKE_EVENTS:-}" ]; then
      printf '%s end %s\n' "$fake_name" "$(node -p '(BigInt(Date.now()) * 1000000n).toString()')" >> "$CODETERM_GATEWAY_FAKE_EVENTS"
    fi
    [ "${4:-success}" != "fail" ] || exit 23
    ;;
  gateway-policy-probe)
    case "${2:-}" in
      integration-owner) require_integration_owner ;;
      release-source) require_release_source ;;
      release-target)
        enter_build_gateway "$CMD ${*:2}"
        prepare_release_target
        printf 'target=%s\nincremental=%s\n' "$CARGO_TARGET_DIR" "$CARGO_INCREMENTAL"
        ;;
      worktree-target)
        enter_build_gateway "$CMD ${*:2}"
        prepare_exceptional_worktree_target
        printf 'target=%s\nincremental=%s\nwrapper=%s\n' "$CARGO_TARGET_DIR" "$CARGO_INCREMENTAL" "$RUSTC_WRAPPER"
        ;;
      qa-child)
        enter_build_gateway "qa-prep"
        bash scripts/qa-prep.sh --gateway-context "$CT_BUILD_JOB_ID" "$$" "$CT_GATEWAY_PROCESS_START" "qa-prep" --probe-only
        ;;
      dev-resources)
        enter_build_gateway "dev-resource-preflight"
        prepare_dev_frontend_dist "${3:?dev-resources requires a fixture root}"
        ;;
      *) echo "Unknown gateway policy probe: ${2:-}" >&2; exit 2 ;;
    esac
    ;;
  check)
    require_integration_owner
    enter_build_gateway "$CMD ${*:2}"
    prepare_dev_frontend_dist "$(pwd -P)"
    prepare_bundle_stubs
    ct_configure_sccache
    cargo check --manifest-path src-tauri/Cargo.toml
    ;;
  test)
    require_integration_owner
    enter_build_gateway "$CMD ${*:2}"
    prepare_dev_frontend_dist "$(pwd -P)"
    prepare_bundle_stubs
    ct_configure_sccache
    scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib
    ;;
  worktree-check)
    enter_build_gateway "$CMD ${*:2}"
    prepare_exceptional_worktree_target
    prepare_bundle_stubs
    cargo check --manifest-path src-tauri/Cargo.toml "${@:2}"
    ;;
  build)
    require_integration_owner
    enter_build_gateway "$CMD ${*:2}"
    require_release_source
    prepare_release_target
    ensure_deps
    ct_configure_sccache
    RELEASE_PROVENANCE="$(ct_gateway_dir)/jobs/$CT_BUILD_JOB_ID.provenance.json"
    ct_snapshot_provenance "full-build" "release" "$(ct_host_triple)" "${RUSTFLAGS:-}" "${CODETERM_BUILD_FEATURES:-}" \
      "$RELEASE_PROVENANCE"
    pnpm tauri build
    ct_check_provenance "full-build" "release" "$(ct_host_triple)" "${RUSTFLAGS:-}" "${CODETERM_BUILD_FEATURES:-}" \
      "$RELEASE_PROVENANCE"
    # Release gate: /web is served from the exe-adjacent dist/ bundle that
    # tauri.conf bundle.resources maps in (../dist -> dist). If Tauri's
    # beforeBuildCommand produced no dist/index.html, /web ships broken.
    if [ ! -f dist/index.html ]; then
      echo "❌ RELEASE GATE: dist/index.html missing after build — /web would 404. Run 'pnpm build'." >&2
      exit 1
    fi
    echo "✅ web bundle present (dist/index.html) — packaged into installer via bundle.resources"
    ;;
  qa-prep)
    require_integration_owner
    enter_build_gateway "$CMD ${*:2}"
    bash scripts/qa-prep.sh --gateway-context "$CT_BUILD_JOB_ID" "$$" "$CT_GATEWAY_PROCESS_START" "$CMD ${*:2}" -- "${@:2}"
    ;;
  bootstrap)
    require_integration_owner
    enter_build_gateway "$CMD ${*:2}"
    ensure_deps
    prepare_bundle_stubs
    ct_configure_sccache
    touch cli/src/main.rs
    cargo build --release --manifest-path cli/Cargo.toml
    CODETERM_DEV=1 node scripts/bundle-cli.js
    cargo build --release --manifest-path src-tauri/Cargo.toml --bin codeterm-embed
    node scripts/bundle-embed.js
    echo "Bootstrap complete: bundled CLI and codeterm-embed are present."
    ;;
  embed)
    require_integration_owner
    enter_build_gateway "$CMD ${*:2}"
    # Force-rebuild the codeterm-embed binary. Skipped on every dev cycle
    # by default — use this when you change embed sources or pull new
    # ONNX/embeddings deps.
    prepare_bundle_stubs
    ct_configure_sccache
    cargo build --release --manifest-path src-tauri/Cargo.toml --bin codeterm-embed
    node scripts/bundle-embed.js
    echo "codeterm-embed rebuilt"
    ;;
  cli)
    require_integration_owner
    enter_build_gateway "$CMD ${*:2}"
    ct_configure_sccache
    touch cli/src/main.rs
    cargo build --release --manifest-path cli/Cargo.toml
    install_cli_to "$HOME/.codeterm/bin"
    install_cli_to "$HOME/.codeterm-dev/bin"
    ;;
  clean)
    require_integration_owner
    enter_build_gateway "$CMD ${*:2}"
    cargo clean --manifest-path src-tauri/Cargo.toml
    ;;
  kill)
    kill_dev
    echo "Done."
    ;;
  dev)
    require_integration_owner
    enter_build_gateway "$CMD ${*:2}"
    prepare_dev_frontend_dist
    kill_dev
    ensure_deps
    ct_configure_sccache
    echo "Building CLI..."
    cargo build --release --manifest-path cli/Cargo.toml
    if [ -z "$CT_INSTANCE" ]; then
      install_cli_to "$HOME/.codeterm/bin"
    else
      echo "Skipping prod CLI install for named dev instance ($CT_INSTANCE)"
    fi
    # Named instance gets its own data-dir bin; canonical dev keeps ~/.codeterm-dev/bin.
    install_cli_to "$CT_DATA_DIR/bin"
    ensure_real_embed_bundle
    echo "Starting dev..."
    # Mark the daemon as dev: bundle hashes are bypassed (live edits
    # to <repo>/plugins/<id>/ don't trip the verifier) and the
    # first-launch embedded extract is skipped so ~/.codeterm/plugins/
    # doesn't shadow the workspace.
    export CODETERM_DEV=1
    export WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="${WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:---remote-debugging-port=$CT_CDP_PORT}"
    start_web_watch

    WAIT=0
    for arg in "${@:2}"; do
      case "$arg" in
        --wait|--wait-build) WAIT=1 ;;
      esac
    done

    if [ "$WAIT" -eq 1 ]; then
      LOG="/tmp/codeterm-dev${CT_SUFFIX}.log"
      : > "$LOG"
      EXPECTED_BUILD_SHA="$(git rev-parse HEAD 2>/dev/null || true)"
      DEV_PID=$(start_dev_wait_process "$LOG")
      echo "Dev pid $DEV_PID, log $LOG. Waiting for /api/health to respond..."
      DEADLINE=$(( $(date +%s) + 600 ))
      while : ; do
        if [ "$(date +%s)" -gt "$DEADLINE" ]; then
          echo "Timeout after 600s. Last log lines:"
          tail -25 "$LOG"
          exit 1
        fi
        if ! dev_wait_process_alive "$DEV_PID"; then
          echo "Dev process exited before reporting ready. Last log lines:"
          tail -25 "$LOG"
          exit 1
        fi
        HEALTH=$(dev_health_status)
        if [ -n "$HEALTH" ]; then
          STATUS=$(printf '%s' "$HEALTH" | dev_health_fields 2>/dev/null | cut -f1 || true)
          if [ "$STATUS" = "ok" ]; then
            if ! SHA=$(validate_dev_health "$EXPECTED_BUILD_SHA" "$CT_INSTANCE" "$CT_PORT" "$CT_VITE_PORT" "$CT_CDP_PORT" "$CT_IDENTIFIER" "$HEALTH"); then
              echo "$HEALTH"
              exit 1
            fi
            STABLE=1
            for _ in 1 2 3 4; do
              sleep 2
              if ! dev_wait_process_alive "$DEV_PID"; then
                STABLE=0
                break
              fi
              if [ -z "$(dev_listener_pid)" ]; then
                STABLE=0
                break
              fi
              HEALTH=$(dev_health_status)
              if ! SHA=$(validate_dev_health "$EXPECTED_BUILD_SHA" "$CT_INSTANCE" "$CT_PORT" "$CT_VITE_PORT" "$CT_CDP_PORT" "$CT_IDENTIFIER" "$HEALTH" 2>/dev/null); then
                STABLE=0
                break
              fi
            done
            if [ "$STABLE" != "1" ]; then
              echo "Dev process became unhealthy immediately after reporting ready. Last log lines:"
              tail -25 "$LOG"
              exit 1
            fi
            echo "Dev ready: build ${SHA:0:8}, instance '${CT_INSTANCE:-default}', launcher pid $DEV_PID, listener pid $(dev_listener_pid)"
            handoff_gateway_to_dev "$DEV_PID"
            exit 0
          fi
        fi
        sleep 2
      done
    else
      pnpm tauri dev --no-watch --config "$(dev_conf_path)"
    fi
    ;;
  e2e)
    export CODETERM_API_URL="http://127.0.0.1:${CT_PORT}"
    cd tests/e2e
    pip install -q -r requirements.txt 2>/dev/null
    python3 -m pytest -m "not llm" -v --tb=short "${@:2}"
    ;;
  e2e:llm)
    export CODETERM_API_URL="http://127.0.0.1:${CT_PORT}"
    cd tests/e2e
    pip install -q -r requirements.txt 2>/dev/null
    python3 -m pytest -m llm -v --tb=short "${@:2}"
    ;;
  test:all)
    require_integration_owner
    enter_build_gateway "$CMD ${*:2}"
    # Aggregate runner for every NON-LLM test layer. Unit layers always run;
    # dev-dependent integration layers (pytest e2e, Playwright @smoke) run only
    # when dev (7686) is healthy, otherwise they SKIP loudly. Exits non-zero iff
    # an EXECUTED layer failed (SKIP never fails the run). LLM-judged layers
    # (evals/, Playwright @golden) are excluded by design — see docs/TESTING.md.
    ta_names=(); ta_status=()
    ta_record() { ta_names+=("$1"); ta_status+=("$2"); }
    ta_run() { # name, cmd...
      local name="$1"; shift
      echo; echo "═══════════ layer: $name ═══════════"
      if "$@"; then ta_record "$name" PASS; else ta_record "$name" FAIL; fi
    }

    ta_run "rust-unit"  cargo test --manifest-path src-tauri/Cargo.toml --lib
    ta_run "ts-unit"    pnpm -s test:unit
    ta_run "plugin"     pnpm -s test:plugins

    DEV_STATUS=$(dev_health_status | python3 -c "import sys,json; print(json.load(sys.stdin).get('status',''))" 2>/dev/null || true)
    if [ "$DEV_STATUS" = "ok" ]; then
      ta_run "e2e-pytest" bash -c "cd tests/e2e && pip install -q -r requirements.txt 2>/dev/null && CODETERM_API_URL=http://127.0.0.1:${CT_PORT} python3 -m pytest -m 'not llm' -q"
      ta_run "playwright-smoke" pnpm -s test:pw:smoke
    else
      echo
      echo ">>> SKIPPED e2e-pytest — dev 7686 not healthy (start with ./build.sh dev)"
      ta_record "e2e-pytest" SKIP
      echo ">>> SKIPPED playwright-smoke — dev 7686 not healthy (start with ./build.sh dev)"
      ta_record "playwright-smoke" SKIP
    fi

    echo; echo "════════════ test:all summary ════════════"
    ta_rc=0
    for i in "${!ta_names[@]}"; do
      printf '  %-18s %s\n' "${ta_names[$i]}" "${ta_status[$i]}"
      [ "${ta_status[$i]}" = "FAIL" ] && ta_rc=1
    done
    echo "──────────────────────────────────────────"
    if [ "$ta_rc" -eq 0 ]; then
      echo "✅ all executed layers passed"
    else
      echo "❌ one or more executed layers failed"
    fi
    exit "$ta_rc"
    ;;
  *)
    echo "Usage: ./build.sh {dev|check|worktree-check|test|test:all|build|qa-prep|cli|clean|kill|e2e|fmt-check|gateway-status|gateway-cancel|gateway-fake|gateway-health-check|env}"
    exit 1
    ;;
esac
