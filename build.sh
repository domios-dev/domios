#!/usr/bin/env bash
# CodeTerm build script — works on macOS, Linux, and Windows.
# Usage:
#   ./build.sh dev       — dev mode (hot reload, incremental)
#   ./build.sh dev --wait — same, but backgrounds and blocks until /api/health
#                          responds (handy for scripts/agents)
#   ./build.sh check     — cargo check only (~2s incremental)
#   ./build.sh test      — run Rust unit tests
#   ./build.sh build     — production build
#   ./build.sh bootstrap — create/build bundled CLI + embed sidecar for fresh clones
#   ./build.sh cli       — build CLI and install to ~/.codeterm/bin/ + ~/.codeterm-dev/bin/
#   ./build.sh clean     — clean build cache
#   ./build.sh kill      — kill dev instance (safe, won't touch prod)
#   ./build.sh e2e       — run non-LLM E2E tests against dev (port 7686)
#   ./build.sh e2e:llm   — run LLM E2E tests against dev (real provider tokens)
#   source build.sh env  — load MSVC env vars into current shell (Windows only)

set -e

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
  kill_web_watch 2>/dev/null || true
  local session_prefix="codeterm-dev${CT_SUFFIX}-"
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    if command -v screen >/dev/null 2>&1; then
      screen -ls 2>/dev/null | awk -v p="$session_prefix" 'index($1,p){print $1}' | while read -r session; do
        screen -S "$session" -X quit 2>/dev/null || true
      done
    fi
    lsof -tiTCP:"$CT_PORT" -sTCP:LISTEN 2>/dev/null | xargs kill 2>/dev/null || true
    lsof -ti:"$CT_VITE_PORT" 2>/dev/null | xargs kill -9 2>/dev/null || true
    if [ -f "$CT_DATA_DIR/daemon.pid" ]; then
      kill "$(cat "$CT_DATA_DIR/daemon.pid")" 2>/dev/null || true
      rm -f "$CT_DATA_DIR/daemon.pid"
    fi
  else
    local dev_pid
    dev_pid=$(netstat -ano 2>/dev/null | grep ":$CT_PORT.*LISTEN" | awk '{print $5}' | head -1)
    if [ -n "$dev_pid" ] && [ "$dev_pid" != "0" ]; then
      echo "Killing dev instance (PID $dev_pid on port $CT_PORT)..."
      taskkill //PID "$dev_pid" //F 2>/dev/null || true
      sleep 1
    fi
    local vite_pid
    vite_pid=$(netstat -ano 2>/dev/null | grep ":$CT_VITE_PORT.*LISTEN" | awk '{print $5}' | head -1)
    if [ -n "$vite_pid" ] && [ "$vite_pid" != "0" ]; then
      taskkill //PID "$vite_pid" //F 2>/dev/null || true
    fi
  fi
}

setup_env

# --- Per-worktree dev instance isolation ---
# CODETERM_INSTANCE (optional) names a dev instance so multiple worktrees can
# run ./build.sh dev concurrently without colliding. Unset = canonical defaults
# (.codeterm-dev / 7686 / 1420). See AGENTS.md "Process safety".
CT_INSTANCE="${CODETERM_INSTANCE:-}"
CT_PORT="${CODETERM_API_PORT:-7686}"
# PROD GUARD: 7685 is the production daemon. build.sh is dev-only; it must NEVER
# target prod, even if a caller inherited CODETERM_API_PORT=7685 from a prod pane.
if [ "$CT_PORT" = "7685" ]; then
  echo "REFUSING: port 7685 is PRODUCTION. build.sh is dev-only and will not kill/start it." >&2
  echo "  (You inherited CODETERM_API_PORT=7685 from a prod pane. Set CODETERM_API_PORT=7686" >&2
  echo "   for the canonical dev, or CODETERM_INSTANCE=<name> CODETERM_API_PORT=<port> for a worktree.)" >&2
  exit 2
fi
if [ -n "$CT_INSTANCE" ]; then
  CT_SLUG=$(printf '%s' "$CT_INSTANCE" | tr '[:upper:]' '[:lower:]' | sed 's/[^a-z0-9-]/-/g')
  CT_SUFFIX="-$CT_SLUG"
  # Stable per-instance vite port: sum of slug bytes mod 200, +1 offset.
  CT_OFFSET=$(printf '%s' "$CT_SLUG" | od -An -tu1 | awk '{for(i=1;i<=NF;i++)s+=$i} END{print (s%200)+1}')
  CT_VITE_PORT=$((1420 + CT_OFFSET))
else
  CT_SLUG=""
  CT_SUFFIX=""
  CT_VITE_PORT=1420
fi
CT_DATA_DIR="$HOME/.codeterm-dev${CT_SUFFIX}"
# Export so the spawned daemon + its child PTYs inherit the instance identity.
export CODETERM_INSTANCE="$CT_INSTANCE"
export CODETERM_API_PORT="$CT_PORT"
export CODETERM_VITE_PORT="$CT_VITE_PORT"

# Wire .githooks as the hooks directory
git config core.hooksPath .githooks 2>/dev/null || true

# Copy the freshly-built CLI into a target bin dir. Both prod (~/.codeterm/bin)
# and dev (~/.codeterm-dev/bin) need the same binary so the dev daemon's
# resolve_cli_path (which prefers data_dir/bin first) doesn't shell into a
# stale CLI that's missing recent subcommands.
install_cli_to() {
  local bin_dir="$1"
  mkdir -p "$bin_dir"
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    cp cli/target/release/codeterm "$bin_dir/codeterm"
  else
    cp cli/target/release/codeterm.exe "$bin_dir/codeterm.exe"
  fi
  echo "CLI installed to $bin_dir"
}

prepare_bundle_stubs() {
  node scripts/prepare-cli-stub.js
  node scripts/prepare-embed-stub.js
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
  node -e "const fs=require('fs');const c=JSON.parse(fs.readFileSync('$base','utf8'));c.build=c.build||{};c.build.devUrl='http://127.0.0.1:'+process.env.CODETERM_VITE_PORT;fs.writeFileSync('$out',JSON.stringify(c,null,2));" >&2
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

# /web browser clients are served the static dist/ bundle, NOT vite HMR.
# Watch-rebuild it so phone/browser mirrors pick up frontend edits on reload.
# Tauri's beforeDevCommand owns the live devUrl server on 1420.
web_watch_pidfile() {
  echo "/tmp/codeterm-web-watch${CT_SUFFIX}.pid"
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
    kill "$(cat "$pidfile")" 2>/dev/null || true
    rm -f "$pidfile"
  fi
}

dev_health_status() {
  curl -s --max-time 2 "http://127.0.0.1:${CT_PORT}/api/health" 2>/dev/null || true
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

CMD="${1:-dev}"

case "$CMD" in
  env)
    # Already loaded above. Use: source build.sh env
    ;;
  check)
    prepare_bundle_stubs
    cargo check --manifest-path src-tauri/Cargo.toml
    ;;
  test)
    prepare_bundle_stubs
    cargo test --manifest-path src-tauri/Cargo.toml --lib
    ;;
  build)
    ensure_deps
    pnpm tauri build
    # Release gate: /web is served from the exe-adjacent dist/ bundle that
    # tauri.conf bundle.resources maps in (../dist -> dist). If pnpm build
    # produced no dist/index.html the installer ships a /web that 404s.
    if [ ! -f dist/index.html ]; then
      echo "❌ RELEASE GATE: dist/index.html missing after build — /web would 404. Run 'pnpm build'." >&2
      exit 1
    fi
    echo "✅ web bundle present (dist/index.html) — packaged into installer via bundle.resources"
    ;;
  bootstrap)
    ensure_deps
    prepare_bundle_stubs
    touch cli/src/main.rs
    cargo build --release --manifest-path cli/Cargo.toml
    CODETERM_DEV=1 node scripts/bundle-cli.js
    cargo build --release --manifest-path src-tauri/Cargo.toml --bin codeterm-embed
    node scripts/bundle-embed.js
    echo "Bootstrap complete: bundled CLI and codeterm-embed are present."
    ;;
  embed)
    # Force-rebuild the codeterm-embed binary. Skipped on every dev cycle
    # by default — use this when you change embed sources or pull new
    # ONNX/embeddings deps.
    prepare_bundle_stubs
    cargo build --release --manifest-path src-tauri/Cargo.toml --bin codeterm-embed
    node scripts/bundle-embed.js
    echo "codeterm-embed rebuilt"
    ;;
  cli)
    touch cli/src/main.rs
    cargo build --release --manifest-path cli/Cargo.toml
    install_cli_to "$HOME/.codeterm/bin"
    install_cli_to "$HOME/.codeterm-dev/bin"
    ;;
  clean)
    cargo clean --manifest-path src-tauri/Cargo.toml
    ;;
  kill)
    kill_dev
    echo "Done."
    ;;
  dev)
    kill_dev
    ensure_deps
    echo "Building CLI..."
    cargo build --release --manifest-path cli/Cargo.toml
    install_cli_to "$HOME/.codeterm/bin"
    # Named instance gets its own data-dir bin; canonical dev keeps ~/.codeterm-dev/bin.
    install_cli_to "$CT_DATA_DIR/bin"
    ensure_real_embed_bundle
    echo "Starting dev..."
    # Mark the daemon as dev: bundle hashes are bypassed (live edits
    # to <repo>/plugins/<id>/ don't trip the verifier) and the
    # first-launch embedded extract is skipped so ~/.codeterm/plugins/
    # doesn't shadow the workspace.
    export CODETERM_DEV=1
    export WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="${WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:---remote-debugging-port=9223}"
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
          SHA=$(printf '%s' "$HEALTH" | python3 -c "import sys,json; print(json.load(sys.stdin).get('build','')[:8])" 2>/dev/null || true)
          STATUS=$(printf '%s' "$HEALTH" | python3 -c "import sys,json; print(json.load(sys.stdin).get('status',''))" 2>/dev/null || true)
          if [ "$STATUS" = "ok" ]; then
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
              STATUS=$(printf '%s' "$HEALTH" | python3 -c "import sys,json; print(json.load(sys.stdin).get('status',''))" 2>/dev/null || true)
              if [ "$STATUS" != "ok" ]; then
                STABLE=0
                break
              fi
            done
            if [ "$STABLE" != "1" ]; then
              echo "Dev process became unhealthy immediately after reporting ready. Last log lines:"
              tail -25 "$LOG"
              exit 1
            fi
            echo "Dev ready: build $SHA, launcher pid $DEV_PID, listener pid $(dev_listener_pid)"
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
    echo "Usage: ./build.sh {dev|check|test|test:all|build|cli|clean|kill|e2e|env}"
    exit 1
    ;;
esac
