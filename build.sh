#!/usr/bin/env bash
# CodeTerm build script — works on macOS, Linux, and Windows.
# Usage:
#   ./build.sh dev       — dev mode (hot reload, incremental)
#   ./build.sh dev --wait — same, but backgrounds and blocks until /api/health
#                          responds (handy for scripts/agents)
#   ./build.sh check     — integration-owner cargo check
#   ./build.sh release-preflight — refresh and verify release Cargo lockfiles
#   ./build.sh worktree-check — explicit isolated, nonincremental lane check
#   ./build.sh build     — production build
#   ./build.sh install-local — incremental production build + self-install, then resume the calling tab
#   ./build.sh bootstrap — create/build bundled CLI + embed sidecar for fresh clones
#   ./build.sh qa-prep   — one-shot: dist + sidecars for a fresh isolated worktree (no tsc gate)
#   ./build.sh cli       — build CLI and install to ~/.codeterm/bin/ + ~/.codeterm-dev/bin/
#                          without waiting for the app/dev build gateway
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
  if [ "${CODETERM_TEST_KILL_CLEAN_REQUEST_ONLY:-0}" = "1" ]; then
    request_dev_clean_restart 2>/dev/null || true
    return
  fi
  local session_prefix="codeterm-dev${CT_SUFFIX}-"
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    local dev_pids
    dev_pids="$(lsof -tiTCP:"$CT_PORT" -sTCP:LISTEN 2>/dev/null || true)"
    if [ -n "$dev_pids" ]; then
      request_dev_clean_restart 2>/dev/null || true
      local graceful=1
      while IFS= read -r dev_pid; do
        wait_for_dev_exit "$dev_pid" || graceful=0
      done <<< "$dev_pids"
      if [ "$graceful" != "1" ]; then
        echo "Clean shutdown deadline elapsed; force-killing dev instance on port $CT_PORT..."
        echo "$dev_pids" | xargs kill -9 2>/dev/null || true
      fi
    fi
    kill_web_watch 2>/dev/null || true
    kill_dev_static_server 2>/dev/null || true
    if command -v screen >/dev/null 2>&1; then
      screen -ls 2>/dev/null | awk -v p="$session_prefix" 'index($1,p){print $1}' | while read -r session; do
        screen -S "$session" -X quit 2>/dev/null || true
      done
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
      request_dev_clean_restart 2>/dev/null || true
      if ! wait_for_dev_exit "$dev_pid"; then
        echo "Clean shutdown deadline elapsed; force-killing dev instance (PID $dev_pid on port $CT_PORT)..."
        taskkill //PID "$dev_pid" //T //F 2>/dev/null || true
      fi
    fi
    kill_web_watch 2>/dev/null || true
    kill_dev_static_server 2>/dev/null || true
    local vite_pid
    vite_pid=$(netstat -ano 2>/dev/null | grep ":$CT_VITE_PORT.*LISTEN" | awk '{print $5}' | head -1 || true)
    if [ -n "$vite_pid" ] && [ "$vite_pid" != "0" ]; then
      taskkill //PID "$vite_pid" //T //F 2>/dev/null || true
    fi
    windows_dev_leftover_cleanup
  fi
}

request_dev_clean_restart() {
  local config="$CT_DATA_DIR/config.toml"
  [ -f "$config" ] || return 1

  local token
  token=$(grep -E '^token = ' "$config" | head -1 | sed 's/.*"\(.*\)"/\1/')
  [ -n "$token" ] || return 1

  curl --fail --silent --show-error --max-time 3 \
    -H "Authorization: Bearer $token" \
    -X POST "http://127.0.0.1:$CT_PORT/api/debug/shutdown/clean-restart" \
    >/dev/null
}

wait_for_dev_exit() {
  local pid="$1"
  if [[ "$OSTYPE" == linux* ]]; then
    timeout 10 bash -c 'while kill -0 "$1" 2>/dev/null; do sleep 0.1; done' -- "$pid"
  elif [[ "$OSTYPE" == darwin* ]]; then
    perl -e '$pid = shift; $SIG{ALRM} = sub { exit 1 }; alarm 10; while (kill 0, $pid) { select undef, undef, undef, 0.1 }' "$pid"
  else
    powershell.exe -NoProfile -Command "Wait-Process -Id $pid -Timeout 10" >/dev/null 2>&1
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
CT_VITE_PORT="${CODETERM_VITE_PORT:-}"
CT_CDP_PORT="${CODETERM_CDP_PORT:-}"
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
  CT_VITE_PORT="${CT_VITE_PORT:-$((1420 + CT_OFFSET))}"
  CT_CDP_PORT="${CT_CDP_PORT:-$((9223 + CT_OFFSET))}"
else
  CT_SLUG=""
  CT_SUFFIX=""
  CT_IDENTIFIER="com.codeterm.dev"
  CT_VITE_PORT="${CT_VITE_PORT:-1420}"
  CT_CDP_PORT="${CT_CDP_PORT:-9223}"
fi
CT_DATA_DIR="$HOME/.codeterm-dev${CT_SUFFIX}"
# Export so the spawned daemon + its child PTYs inherit the instance identity.
export CODETERM_INSTANCE="$CT_INSTANCE"
export CODETERM_API_PORT="$CT_PORT"
export CODETERM_VITE_PORT="$CT_VITE_PORT"
export CODETERM_DEV_IDENTIFIER="$CT_IDENTIFIER"
export CODETERM_CDP_PORT="$CT_CDP_PORT"

CT_DEV_WAIT=0
if [ "$CMD" = "dev" ]; then
  for arg in "${@:2}"; do
    case "$arg" in
      --wait|--wait-build) CT_DEV_WAIT=1 ;;
      --release-gateway-after-ready)
        echo "ERROR: --release-gateway-after-ready is obsolete; named dev --wait always skips the shared gateway handoff" >&2
        exit 2
        ;;
    esac
  done
fi

named_dev_target_dir() {
  printf '%s\n' "$CT_DATA_DIR/target"
}

named_dev_wait_isolated() {
  [ -n "$CT_INSTANCE" ] && [ "$CT_DEV_WAIT" = "1" ]
}

prepare_named_dev_target() {
  export CARGO_TARGET_DIR="$(named_dev_target_dir)"
  export CODETERM_ALLOW_EXTERNAL_TARGET=1
  mkdir -p "$CARGO_TARGET_DIR"
  ct_verify_target_dir
  echo "[isolated-dev] cargo target=$CARGO_TARGET_DIR"
}

if [ "${CODETERM_TEST_PRINT_DEV_GATEWAY_POLICY:-0}" = "1" ]; then
  if [ -n "$CT_INSTANCE" ]; then
    printf 'instance=%s\nshared_gateway_handoff=skipped\n' "$CT_INSTANCE"
  else
    printf 'instance=\nshared_gateway_handoff=required\n'
  fi
  exit 0
fi

if [ "${CODETERM_TEST_PRINT_ISOLATED_DEV_POLICY:-0}" = "1" ]; then
  if named_dev_wait_isolated; then
    printf 'mode=isolated-named-wait\ntarget=%s\nbuild_gateway=skipped\n' "$(named_dev_target_dir)"
  else
    printf 'mode=shared\nbuild_gateway=required\n'
  fi
  exit 0
fi

# Wire .githooks as the hooks directory
git config core.hooksPath .githooks 2>/dev/null || true

source scripts/cache-policy.sh
source scripts/build-gateway.sh
source scripts/release-signing.sh

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

prepare_local_install_target() {
  local dir canonical current identity
  current="$(git rev-parse --show-toplevel)"
  canonical="$(git worktree list --porcelain | awk '/^worktree /{print $2; exit}')"
  if [ "$current" != "$canonical" ]; then
    echo "ERROR: install-local must run from the canonical checkout: $canonical" >&2
    exit 2
  fi
  identity="$(printf '%s' "$canonical" | git hash-object --stdin)"
  dir="$(ct_gateway_dir)/targets/local-install/$identity"
  export CARGO_TARGET_DIR="$dir"
  export CARGO_INCREMENTAL=0
  export CODETERM_ALLOW_EXTERNAL_TARGET=1
  mkdir -p "$CARGO_TARGET_DIR"
  ct_verify_target_dir
  echo "[cache-policy] persistent local-install target=$CARGO_TARGET_DIR"
  echo "[cache-policy] CARGO_INCREMENTAL=$CARGO_INCREMENTAL"
}

parse_local_install_continuation() {
  LOCAL_INSTALL_CONTINUE_TAB="${CODETERM_TAB_ID:-}"
  LOCAL_INSTALL_CONTINUE_PROMPT=""
  LOCAL_INSTALL_CONTINUE_ENABLED=1

  while [ "$#" -gt 0 ]; do
    case "$1" in
      --continue-prompt)
        [ "$#" -ge 2 ] || { echo "ERROR: --continue-prompt requires text" >&2; exit 2; }
        LOCAL_INSTALL_CONTINUE_PROMPT="$2"
        shift 2
        ;;
      --continue-tab)
        [ "$#" -ge 2 ] || { echo "ERROR: --continue-tab requires a Tab id" >&2; exit 2; }
        LOCAL_INSTALL_CONTINUE_TAB="$2"
        shift 2
        ;;
      --no-continue)
        LOCAL_INSTALL_CONTINUE_ENABLED=0
        shift
        ;;
      *)
        echo "ERROR: unknown install-local option: $1" >&2
        exit 2
        ;;
    esac
  done

  if [ "$LOCAL_INSTALL_CONTINUE_ENABLED" = "0" ]; then
    LOCAL_INSTALL_CONTINUE_TAB=""
    LOCAL_INSTALL_CONTINUE_PROMPT=""
    return
  fi

  if [ -z "$LOCAL_INSTALL_CONTINUE_TAB" ]; then
    if [ -n "$LOCAL_INSTALL_CONTINUE_PROMPT" ]; then
      echo "ERROR: install-local continuation needs --continue-tab or CODETERM_TAB_ID" >&2
      exit 2
    fi
    LOCAL_INSTALL_CONTINUE_ENABLED=0
    return
  fi

  if [ -z "$LOCAL_INSTALL_CONTINUE_PROMPT" ]; then
    local source_sha
    source_sha="$(git rev-parse HEAD)"
    LOCAL_INSTALL_CONTINUE_PROMPT="Local CodeTerm install completed for build $source_sha. Verify the production health endpoint reports this SHA, then continue the work that requested the reinstall."
  fi
}

require_release_source() {
  local actual expected dirty
  actual="$(git rev-parse HEAD)"
  expected="${CODETERM_EXPECTED_BUILD_SHA:-$actual}"
  if [ "$actual" != "$expected" ]; then
    echo "ERROR: release source mismatch: expected $expected, got $actual" >&2
    exit 2
  fi
  dirty="$(
    {
      git diff --name-only HEAD --
      git ls-files --others --exclude-standard
    } | sort -u
  )"
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

write_cli_docs() {
  local release_dir binary
  release_dir="$(cli_release_dir)"
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    binary="$release_dir/codeterm"
  else
    binary="$release_dir/codeterm.exe"
  fi
  "$binary" cli-manifest --write
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
# whose devUrl points at this instance's frontend static port.
dev_conf_path() {
  local base="src-tauri/tauri.dev.conf.json"
  if [ -z "$CT_SUFFIX" ]; then
    echo "$base"
    return
  fi
  local out="src-tauri/tauri.dev.conf${CT_SUFFIX}.json"
  node -e "const fs=require('fs');const c=JSON.parse(fs.readFileSync('$base','utf8'));const defaults='--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection';const cdp='--remote-debugging-port=$CT_CDP_PORT';let args=process.env.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS||cdp;if(!args.includes('--remote-debugging-port='))args=[args,cdp].filter(Boolean).join(' ');if(!args.includes('--disable-features='))args=[defaults,args].filter(Boolean).join(' ');c.identifier=process.env.CODETERM_DEV_IDENTIFIER;c.build=c.build||{};c.build.devUrl='http://127.0.0.1:'+process.env.CODETERM_VITE_PORT;c.app=c.app||{};c.app.windows=c.app.windows&&c.app.windows.length?c.app.windows:[{}];c.app.windows[0]={...c.app.windows[0],additionalBrowserArgs:args};fs.writeFileSync('$out',JSON.stringify(c,null,2));" >&2
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
    CODETERM_DEV=1 node scripts/bundle-embed.js
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
  local pid spawn_status
  set +e
  case "$OSTYPE" in
    msys*|cygwin*|win32)
      "${cmd[@]}" </dev/null >> "$log" 2>&1 &
      ;;
    *)
      nohup "${cmd[@]}" </dev/null >> "$log" 2>&1 &
      ;;
  esac
  pid=$!
  spawn_status=$?
  set -e
  if [ -z "$pid" ]; then
    echo "Failed to spawn dev process (status=$spawn_status)" >&2
    return "${spawn_status:-1}"
  fi
  echo "$pid"
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

dev_wait_stage() {
  local log="$1" stage
  stage="$(tail -80 "$log" 2>/dev/null | sed '/^[[:space:]]*$/d' | tail -n1 || true)"
  if [ -n "$stage" ]; then
    printf '%s\n' "$stage"
  else
    printf '%s\n' 'launcher spawned; waiting for compiler or daemon output'
  fi
}

handoff_gateway_to_dev() {
  local dev_pid="$1" monitor_pid monitor_start lock
  lock="$(ct_gateway_dir)/lock"
  mkdir -p "$lock"
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
web_watch_pidfile() {
  echo "/tmp/codeterm-web-watch${CT_SUFFIX}.pid"
}

dev_static_pidfile() {
  echo "/tmp/codeterm-dev-static${CT_SUFFIX}.pid"
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

kill_orphan_dev_static_servers() {
  if [[ "$OSTYPE" != darwin* ]] && [[ "$OSTYPE" != linux* ]]; then
    return
  fi
  local root pid cwd
  root="$(pwd)"
  for pid in $(pgrep -f "scripts/dev-static-server.mjs" 2>/dev/null || true); do
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

  local root_win target_debug_win data_dir_win cleanup_script ps
  root_win="$(pwd -W 2>/dev/null || pwd)"
  target_debug_win="$(cd "$(tauri_debug_dir)" 2>/dev/null && pwd -W 2>/dev/null || echo "$(pwd -W 2>/dev/null || pwd)/$(tauri_debug_dir)")"
  data_dir_win="$(cd "$CT_DATA_DIR" 2>/dev/null && pwd -W 2>/dev/null || cygpath -w "$CT_DATA_DIR" 2>/dev/null || echo "$CT_DATA_DIR")"
  cleanup_script="$root_win/scripts/windows-dev-cleanup.ps1"
  if command -v powershell.exe >/dev/null 2>&1; then
    ps="powershell.exe"
  else
    ps="powershell"
  fi
  "$ps" -NoProfile -ExecutionPolicy Bypass -File "$cleanup_script" -Root "$root_win" \
    -TargetDebug "$target_debug_win" -DataDir "$data_dir_win" -Identifier "$CT_IDENTIFIER" -Instance "$CT_INSTANCE" \
    -VitePort "$CT_VITE_PORT" -ApiPort "$CT_PORT" -CdpPort "$CT_CDP_PORT" 2>/dev/null || true
}

start_web_watch() {
  kill_web_watch || true
  local log="/tmp/codeterm-web-watch${CT_SUFFIX}.log"
  local pid spawn_status
  set +e
  case "$OSTYPE" in
    msys*|cygwin*|win32)
      CODETERM_FRONTEND_DEV=1 pnpm exec vite build --watch </dev/null >> "$log" 2>&1 &
      ;;
    *)
      CODETERM_FRONTEND_DEV=1 nohup pnpm exec vite build --watch </dev/null >> "$log" 2>&1 &
      ;;
  esac
  pid=$!
  spawn_status=$?
  set -e
  if [ -z "$pid" ] || [ "$spawn_status" -ne 0 ]; then
    echo "WARNING: web bundle watcher did not start (status=$spawn_status, log $log); continuing without dist/ auto-rebuilds." >&2
    return 0
  fi
  if ! echo "$pid" > "$(web_watch_pidfile)"; then
    echo "WARNING: web bundle watcher pidfile was not written; continuing without managed watcher cleanup." >&2
    return 0
  fi
  echo "Web bundle watcher started (dist/ auto-rebuilds, pid $pid, log $log)"
}

start_dev_static_server() {
  kill_dev_static_server || true
  local log="/tmp/codeterm-dev-static${CT_SUFFIX}.log"
  local pid spawn_status
  set +e
  case "$OSTYPE" in
    msys*|cygwin*|win32)
      node scripts/dev-static-server.mjs </dev/null >> "$log" 2>&1 &
      ;;
    *)
      nohup node scripts/dev-static-server.mjs </dev/null >> "$log" 2>&1 &
      ;;
  esac
  pid=$!
  spawn_status=$?
  set -e
  if [ -z "$pid" ] || [ "$spawn_status" -ne 0 ]; then
    echo "ERROR: dev static server did not start (status=$spawn_status, log $log)" >&2
    return 1
  fi
  echo "$pid" > "$(dev_static_pidfile)"
  echo "Dev static server started (devUrl port $CT_VITE_PORT, pid $pid, log $log)"
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

kill_dev_static_server() {
  local pidfile
  pidfile="$(dev_static_pidfile)"
  if [ -f "$pidfile" ]; then
    kill_process_tree "$(cat "$pidfile")"
    rm -f "$pidfile"
  fi
  kill_orphan_dev_static_servers
}

dev_health_status() {
  curl -s --max-time 2 "http://127.0.0.1:${CT_PORT}/api/health" 2>/dev/null || true
}

dev_frontend_ready() {
  if [[ "$OSTYPE" != darwin* ]] && [[ "$OSTYPE" != linux* ]]; then
    node scripts/dev-frontend-ready.mjs ready
    return
  fi
  curl -fs --max-time 3 "http://127.0.0.1:${CT_VITE_PORT}/" 2>/dev/null | grep -q "<div id=\"root\""
}

dev_frontend_snapshot() {
  if [[ "$OSTYPE" != darwin* ]] && [[ "$OSTYPE" != linux* ]]; then
    node scripts/dev-frontend-ready.mjs snapshot || true
    return
  fi
  if curl -fs --max-time 3 "http://127.0.0.1:${CT_VITE_PORT}/" >/tmp/codeterm-frontend-snapshot.html 2>/dev/null; then
    echo "frontend: vite returned HTML" >&2
  else
    echo "frontend: vite did not answer" >&2
  fi
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
  local status sha instance port vite cdp identifier fields
  fields="$(printf '%s' "$health" | dev_health_fields 2>/dev/null || true)"
  status="$(printf '%s\n' "$fields" | sed -n '1p')"
  sha="$(printf '%s\n' "$fields" | sed -n '2p')"
  instance="$(printf '%s\n' "$fields" | sed -n '3p')"
  port="$(printf '%s\n' "$fields" | sed -n '4p')"
  vite="$(printf '%s\n' "$fields" | sed -n '5p')"
  cdp="$(printf '%s\n' "$fields" | sed -n '6p')"
  identifier="$(printf '%s\n' "$fields" | sed -n '7p')"
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
    enter_build_gateway "$CMD ${*:2}"
    prepare_dev_frontend_dist "$(pwd -P)"
    prepare_bundle_stubs
    ct_configure_sccache
    cargo check --manifest-path src-tauri/Cargo.toml
    ;;
  release-preflight)
    enter_build_gateway "$CMD ${*:2}"
    prepare_dev_frontend_dist "$(pwd -P)"
    prepare_bundle_stubs
    ct_configure_sccache
    cargo check --manifest-path src-tauri/Cargo.toml
    cargo check --manifest-path cli/Cargo.toml
    node scripts/check-release-locks.mjs
    ;;
  test)
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
    enter_build_gateway "$CMD ${*:2}"
    node scripts/check-release-locks.mjs
    require_release_source
    ct_load_release_signing_key
    prepare_release_target
    ensure_deps
    ct_configure_sccache
    RELEASE_PROVENANCE="$(ct_gateway_dir)/jobs/$CT_BUILD_JOB_ID.provenance.json"
    ct_snapshot_provenance "full-build-input" "release" "$(ct_host_triple)" "${RUSTFLAGS:-}" "${CODETERM_BUILD_FEATURES:-}" \
      "$RELEASE_PROVENANCE"
    pnpm tauri build
    ct_check_provenance "full-build-input" "release" "$(ct_host_triple)" "${RUSTFLAGS:-}" "${CODETERM_BUILD_FEATURES:-}" \
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
  install-local)
    parse_local_install_continuation "${@:2}"
    if [ "${CODETERM_TEST_PRINT_LOCAL_INSTALL_CONTINUATION:-0}" = "1" ]; then
      printf 'enabled=%s\ntab=%s\nprompt=%s\n' \
        "$LOCAL_INSTALL_CONTINUE_ENABLED" \
        "$LOCAL_INSTALL_CONTINUE_TAB" \
        "$LOCAL_INSTALL_CONTINUE_PROMPT"
      exit 0
    fi
    case "$OSTYPE" in
      msys*|cygwin*|win32*) ;;
      *)
        echo "ERROR: install-local is supported only on Windows" >&2
        exit 2
        ;;
    esac
    enter_build_gateway "$CMD ${*:2}"
    node scripts/check-release-locks.mjs
    require_release_source
    ct_load_release_signing_key
    prepare_local_install_target
    ensure_deps
    ct_configure_sccache
    RELEASE_PROVENANCE="$(ct_gateway_dir)/jobs/$CT_BUILD_JOB_ID.provenance.json"
    ct_snapshot_provenance "full-build-input" "local-install" "$(ct_host_triple)" "${RUSTFLAGS:-}" "${CODETERM_BUILD_FEATURES:-}" \
      "$RELEASE_PROVENANCE"
    rm -rf "$CARGO_TARGET_DIR/release/bundle/nsis"
    pnpm tauri build
    ct_check_provenance "full-build-input" "local-install" "$(ct_host_triple)" "${RUSTFLAGS:-}" "${CODETERM_BUILD_FEATURES:-}" \
      "$RELEASE_PROVENANCE"
    VERSION="$(node -p "require('./package.json').version")"
    INSTALLER="$CARGO_TARGET_DIR/release/bundle/nsis/CodeTerm_${VERSION}_x64-setup.exe"
    if [ ! -f "$INSTALLER" ]; then
      echo "ERROR: local installer missing after build: $INSTALLER" >&2
      exit 1
    fi
    echo "Local installer ready: $INSTALLER"
    INSTALL_ARGS=(update install --from-file "$(cygpath -w "$INSTALLER")")
    if [ "$LOCAL_INSTALL_CONTINUE_ENABLED" = "1" ]; then
      INSTALL_ARGS+=(--continue-prompt "$LOCAL_INSTALL_CONTINUE_PROMPT" --continue-tab "$LOCAL_INSTALL_CONTINUE_TAB")
      echo "Post-install continuation: Tab $LOCAL_INSTALL_CONTINUE_TAB"
    else
      echo "Post-install continuation: disabled (no calling CodeTerm Tab)"
    fi
    env \
      -u CODETERM_INSTANCE \
      -u CODETERM_VITE_PORT \
      -u CODETERM_DEV_IDENTIFIER \
      -u CODETERM_CDP_PORT \
      -u CODETERM_ALLOW_EXTERNAL_TARGET \
      -u CARGO_TARGET_DIR \
      -u CARGO_INCREMENTAL \
      -u RUSTC_WRAPPER \
      -u SCCACHE_BASEDIRS \
      codeterm "${INSTALL_ARGS[@]}"
    ;;
  qa-prep)
    enter_build_gateway "$CMD ${*:2}"
    bash scripts/qa-prep.sh --gateway-context "$CT_BUILD_JOB_ID" "$$" "$CT_GATEWAY_PROCESS_START" "$CMD ${*:2}" -- "${@:2}"
    ;;
  bootstrap)
    enter_build_gateway "$CMD ${*:2}"
    ensure_deps
    prepare_bundle_stubs
    ct_configure_sccache
    touch cli/src/main.rs
    cargo build --release --manifest-path cli/Cargo.toml
    CODETERM_DEV=1 node scripts/bundle-cli.js
    cargo build --release --manifest-path src-tauri/Cargo.toml --bin codeterm-embed
    CODETERM_DEV=1 node scripts/bundle-embed.js
    echo "Bootstrap complete: bundled CLI and codeterm-embed are present."
    ;;
  embed)
    enter_build_gateway "$CMD ${*:2}"
    # Force-rebuild the codeterm-embed binary. Skipped on every dev cycle
    # by default — use this when you change embed sources or pull new
    # ONNX/embeddings deps.
    prepare_bundle_stubs
    ct_configure_sccache
    cargo build --release --manifest-path src-tauri/Cargo.toml --bin codeterm-embed
    CODETERM_DEV=1 node scripts/bundle-embed.js
    echo "codeterm-embed rebuilt"
    ;;
  cli)
    echo "[build] CLI uses Cargo's cli/target lock; app/dev gateway is independent."
    ct_configure_sccache
    touch cli/src/main.rs
    cargo build --release --manifest-path cli/Cargo.toml
    write_cli_docs
    install_cli_to "$HOME/.codeterm/bin"
    install_cli_to "$HOME/.codeterm-dev/bin"
    ;;
  clean)
    enter_build_gateway "$CMD ${*:2}"
    cargo clean --manifest-path src-tauri/Cargo.toml
    ;;
  kill)
    kill_dev
    echo "Done."
    ;;
  dev)
    if named_dev_wait_isolated; then
      prepare_named_dev_target
      echo "[isolated-dev] named --wait startup bypasses the canonical dev gateway"
    else
      kill_dev || echo "WARNING: dev cleanup failed; continuing to build gateway acquisition." >&2
      enter_build_gateway "$CMD ${*:2}"
    fi
    prepare_dev_frontend_dist
    ensure_deps
    ct_configure_sccache
    echo "Building CLI..."
    cargo build --release --manifest-path cli/Cargo.toml
    if [ -z "$CT_INSTANCE" ]; then
      if ! install_cli_to "$HOME/.codeterm/bin"; then
        echo "WARNING: shared CLI is in use; continuing with the isolated dev CLI install." >&2
      fi
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
    export CODETERM_WEB_DIST="$(pwd -P)/dist"
    export WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="${WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:---remote-debugging-port=$CT_CDP_PORT}"
    start_web_watch
    start_dev_static_server

    if [ "$CT_DEV_WAIT" -eq 1 ]; then
      LOG="/tmp/codeterm-dev${CT_SUFFIX}.log"
      : > "$LOG"
      EXPECTED_BUILD_SHA="$(git rev-parse HEAD 2>/dev/null || true)"
      DEV_PID=$(start_dev_wait_process "$LOG")
      echo "Dev pid $DEV_PID, log $LOG. Waiting for /api/health to respond..."
      DEADLINE=$(( $(date +%s) + 600 ))
      LAST_STAGE=""
      while : ; do
        STAGE="$(dev_wait_stage "$LOG")"
        if [ "$STAGE" != "$LAST_STAGE" ]; then
          echo "Dev startup stage: $STAGE"
          LAST_STAGE="$STAGE"
        fi
        echo "Dev startup heartbeat: stage=$LAST_STAGE"
        if [ "$(date +%s)" -gt "$DEADLINE" ]; then
          echo "Timeout after 600s during stage: $LAST_STAGE. Last log lines:"
          tail -25 "$LOG"
          if [ -f "/tmp/codeterm-web-watch${CT_SUFFIX}.log" ]; then
            echo "Last web-watch log lines:"
            tail -25 "/tmp/codeterm-web-watch${CT_SUFFIX}.log"
          fi
          exit 1
        fi
        if ! dev_wait_process_alive "$DEV_PID"; then
          echo "Dev process exited before reporting ready during stage: $LAST_STAGE. Last log lines:"
          tail -25 "$LOG"
          exit 1
        fi
        HEALTH=$(dev_health_status)
        if [ -n "$HEALTH" ]; then
          STATUS=$(printf '%s' "$HEALTH" | dev_health_fields 2>/dev/null | head -n1 || true)
          if [ "$STATUS" = "ok" ]; then
            if ! SHA=$(validate_dev_health "$EXPECTED_BUILD_SHA" "$CT_INSTANCE" "$CT_PORT" "$CT_VITE_PORT" "$CT_CDP_PORT" "$CT_IDENTIFIER" "$HEALTH"); then
              echo "$HEALTH"
              exit 1
            fi
            STABLE=1
            for _ in 1 2 3 4; do
              sleep 2
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
            if ! dev_frontend_ready; then
              sleep 2
              continue
            fi
            echo "Dev ready: build ${SHA:0:8}, instance '${CT_INSTANCE:-default}', launcher pid $DEV_PID, listener pid $(dev_listener_pid)"
            # Named --wait startup never acquires the shared gateway, so it must
            # not claim its canonical lock after readiness.
            if [ -n "$CT_INSTANCE" ]; then
              echo "Named dev ready; shared build gateway handoff skipped."
            else
              handoff_gateway_to_dev "$DEV_PID"
            fi
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
    enter_build_gateway "$CMD ${*:2}"
    # Aggregate runner for every enabled NON-LLM test layer. Unit layers always run;
    # dev-dependent API integration tests run only
    # when dev (7686) is healthy, otherwise they SKIP loudly. Exits non-zero iff
    # an EXECUTED layer failed (SKIP never fails the run). Behaviour benches and
    # the suspended Playwright suite are excluded — see docs/TESTING.md.
    ta_names=(); ta_status=()
    ta_record() { ta_names+=("$1"); ta_status+=("$2"); }
    ta_run() { # name, cmd...
      local name="$1"; shift
      echo; echo "═══════════ layer: $name ═══════════"
      if "$@"; then ta_record "$name" PASS; else ta_record "$name" FAIL; fi
    }

    ta_run "rust-unit"  scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib
    ta_run "ts-unit"    pnpm -s test:unit
    ta_run "plugin"     pnpm -s test:plugins

    DEV_STATUS=$(dev_health_status | python3 -c "import sys,json; print(json.load(sys.stdin).get('status',''))" 2>/dev/null || true)
    if [ "$DEV_STATUS" = "ok" ]; then
      ta_run "e2e-pytest" bash -c "cd tests/e2e && pip install -q -r requirements.txt 2>/dev/null && CODETERM_API_URL=http://127.0.0.1:${CT_PORT} python3 -m pytest -m 'not llm' -q"
    else
      echo
      echo ">>> SKIPPED e2e-pytest — dev 7686 not healthy (start with ./build.sh dev)"
      ta_record "e2e-pytest" SKIP
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
    echo "Usage: ./build.sh {dev|check|worktree-check|test|test:all|build|install-local [--continue-prompt TEXT] [--continue-tab ID] [--no-continue]|qa-prep|cli|clean|kill|e2e|fmt-check|gateway-status|gateway-cancel|gateway-fake|gateway-health-check|env}"
    exit 1
    ;;
esac
