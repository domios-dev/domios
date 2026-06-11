#!/usr/bin/env bash
# CodeTerm build script — works on macOS, Linux, and Windows.
# Usage:
#   ./build.sh dev       — dev mode (hot reload, incremental)
#   ./build.sh dev --wait — same, but backgrounds and blocks until /api/health
#                          responds (handy for scripts/agents)
#   ./build.sh check     — cargo check only (~2s incremental)
#   ./build.sh test      — run Rust unit tests
#   ./build.sh build     — production build
#   ./build.sh cli       — build CLI and install to ~/.codeterm/bin/ + ~/.codeterm-dev/bin/
#   ./build.sh clean     — clean build cache
#   ./build.sh kill      — kill dev instance (safe, won't touch prod)
#   ./build.sh webdriver — dev with WebDriver for UI tests
#   ./build.sh e2e       — run E2E tests against dev (port 7686)
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

# --- Kill dev instance only (never prod) ---
kill_dev() {
  kill_web_watch 2>/dev/null || true
  if [[ "$OSTYPE" == darwin* ]]; then
    pgrep -f "target/debug/codeterm" | xargs kill 2>/dev/null || true
    pgrep -f "pnpm tauri dev --no-watch --config src-tauri/tauri.dev.conf.json" | xargs kill 2>/dev/null || true
    lsof -ti:1420 | xargs kill -9 2>/dev/null || true
  else
    local dev_pid
    dev_pid=$(netstat -ano 2>/dev/null | grep ":7686.*LISTEN" | awk '{print $5}' | head -1)
    if [ -n "$dev_pid" ] && [ "$dev_pid" != "0" ]; then
      echo "Killing dev instance (PID $dev_pid on port 7686)..."
      taskkill //PID "$dev_pid" //F 2>/dev/null || true
      sleep 1
    fi
  fi
}

setup_env

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

start_dev_wait_process() {
  local log="$1"
  local cmd=(pnpm tauri dev --no-watch --config src-tauri/tauri.dev.conf.json)
  if [[ "$OSTYPE" == darwin* ]] && command -v script >/dev/null 2>&1; then
    # macOS Tauri dev is more reliable with a controlling pseudo-tty. Plain
    # `nohup pnpm tauri dev` can report healthy, then exit as the launcher shell
    # tears down; `script` keeps the dev command in a pty while still detaching.
    nohup script -q "$log" "${cmd[@]}" </dev/null >/dev/null 2>&1 &
  else
    nohup "${cmd[@]}" </dev/null >> "$log" 2>&1 &
  fi
  echo $!
}

# /web browser clients are served the static dist/ bundle, NOT vite HMR.
# Watch-rebuild it so phone/browser mirrors pick up frontend edits on reload.
start_web_watch() {
  kill_web_watch
  nohup pnpm exec vite build --watch </dev/null >> /tmp/codeterm-web-watch.log 2>&1 &
  echo $! > /tmp/codeterm-web-watch.pid
  echo "Web bundle watcher started (dist/ auto-rebuilds, log /tmp/codeterm-web-watch.log)"
}

kill_web_watch() {
  if [ -f /tmp/codeterm-web-watch.pid ]; then
    kill "$(cat /tmp/codeterm-web-watch.pid)" 2>/dev/null || true
    rm -f /tmp/codeterm-web-watch.pid
  fi
  if [[ "$OSTYPE" == darwin* ]]; then
    pgrep -f "vite build --watch" | xargs kill 2>/dev/null || true
  fi
}

dev_health_status() {
  curl -s --max-time 2 http://127.0.0.1:7686/api/health 2>/dev/null || true
}

dev_listener_pid() {
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    lsof -tiTCP:7686 -sTCP:LISTEN 2>/dev/null | head -1 || true
  else
    netstat -ano 2>/dev/null | grep ":7686.*LISTEN" | awk '{print $5}' | head -1 || true
  fi
}

# Auto-install node deps if lockfile is newer than node_modules
ensure_deps() {
  if [ ! -d node_modules ] || [ pnpm-lock.yaml -nt node_modules/.package-lock.json ] 2>/dev/null; then
    echo "Installing dependencies..."
    pnpm install --frozen-lockfile 2>/dev/null || pnpm install
  fi
}

CMD="${1:-dev}"

case "$CMD" in
  env)
    # Already loaded above. Use: source build.sh env
    ;;
  check)
    cargo check --manifest-path src-tauri/Cargo.toml
    ;;
  test)
    cargo test --manifest-path src-tauri/Cargo.toml --lib
    ;;
  build)
    ensure_deps
    pnpm tauri build
    ;;
  embed)
    # Force-rebuild the codeterm-embed binary. Skipped on every dev cycle
    # by default — use this when you change embed sources or pull new
    # ONNX/embeddings deps.
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
    install_cli_to "$HOME/.codeterm-dev/bin"
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
      LOG=/tmp/codeterm-dev.log
      : > "$LOG"
      DEV_PID=$(start_dev_wait_process "$LOG")
      echo "Dev pid $DEV_PID, log $LOG. Waiting for /api/health to respond..."
      DEADLINE=$(( $(date +%s) + 300 ))
      while : ; do
        if [ "$(date +%s)" -gt "$DEADLINE" ]; then
          echo "Timeout after 300s. Last log lines:"
          tail -25 "$LOG"
          exit 1
        fi
        if ! kill -0 "$DEV_PID" 2>/dev/null; then
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
              if ! kill -0 "$DEV_PID" 2>/dev/null; then
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
      pnpm tauri dev --no-watch --config src-tauri/tauri.dev.conf.json
    fi
    ;;
  webdriver)
    kill_dev
    ensure_deps
    echo "Starting dev with WebDriver..."
    export WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="${WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:---remote-debugging-port=9223}"
    pnpm tauri dev --no-watch --config src-tauri/tauri.dev.conf.json -- --features webdriver
    ;;
  e2e)
    export CODETERM_API_URL="http://127.0.0.1:7686"
    cd tests/e2e
    pip install -q -r requirements.txt 2>/dev/null
    python -m pytest -v --tb=short "${@:2}"
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

    DEV_STATUS=$(dev_health_status | python -c "import sys,json; print(json.load(sys.stdin).get('status',''))" 2>/dev/null || true)
    if [ "$DEV_STATUS" = "ok" ]; then
      ta_run "e2e-pytest" bash -c 'cd tests/e2e && pip install -q -r requirements.txt 2>/dev/null && CODETERM_API_URL=http://127.0.0.1:7686 python -m pytest -q'
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
    echo "Usage: ./build.sh {dev|check|test|test:all|build|cli|clean|kill|webdriver|e2e|env}"
    exit 1
    ;;
esac
