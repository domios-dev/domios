#!/usr/bin/env bash
# CodeTerm build script — works on macOS, Linux, and Windows.
# Usage:
#   ./build.sh dev       — dev mode (hot reload, incremental)
#   ./build.sh dev --wait — same, but backgrounds and blocks until /api/health
#                          reports the new build_number (handy for scripts/agents)
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
    MSVC_VER="14.44.35207"
    BT_BASE="C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools"
    SDK_VER="10.0.26100.0"
    SDK_BASE="C:/Program Files (x86)/Windows Kits/10"
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
  if [[ "$OSTYPE" == darwin* ]]; then
    pgrep -f "target/debug/codeterm" | xargs kill 2>/dev/null || true
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
    # Increment build number
    BUILD_NUM=$(cat BUILD_NUMBER 2>/dev/null || echo 0)
    BUILD_NUM=$((BUILD_NUM + 1))
    echo "$BUILD_NUM" > BUILD_NUMBER
    # Build CLI first (so it's ready when app starts)
    echo "Building CLI..."
    cargo build --release --manifest-path cli/Cargo.toml
    install_cli_to "$HOME/.codeterm/bin"
    install_cli_to "$HOME/.codeterm-dev/bin"
    echo "Build #$BUILD_NUM starting..."

    WAIT=0
    for arg in "${@:2}"; do
      case "$arg" in
        --wait|--wait-build) WAIT=1 ;;
      esac
    done

    if [ "$WAIT" -eq 1 ]; then
      LOG=/tmp/codeterm-dev.log
      : > "$LOG"
      ( pnpm tauri dev --no-watch --config src-tauri/tauri.dev.conf.json >> "$LOG" 2>&1 ) &
      DEV_PID=$!
      disown "$DEV_PID" 2>/dev/null || true
      echo "Dev pid $DEV_PID, log $LOG. Waiting for /api/health to report build_number=$BUILD_NUM..."
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
        HEALTH=$(curl -s --max-time 2 http://127.0.0.1:7686/api/health 2>/dev/null || true)
        if [ -n "$HEALTH" ]; then
          BN=$(printf '%s' "$HEALTH" | python3 -c "import sys,json; print(json.load(sys.stdin).get('build_number',''))" 2>/dev/null || true)
          SHA=$(printf '%s' "$HEALTH" | python3 -c "import sys,json; print(json.load(sys.stdin).get('build','')[:8])" 2>/dev/null || true)
          if [ "$BN" = "$BUILD_NUM" ]; then
            echo "Dev ready: build #$BN (commit $SHA), pid $DEV_PID"
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
    pnpm tauri dev --no-watch --config src-tauri/tauri.dev.conf.json -- --features webdriver
    ;;
  e2e)
    export CODETERM_API_URL="http://127.0.0.1:7686"
    cd tests/e2e
    pip install -q -r requirements.txt 2>/dev/null
    python -m pytest -v --tb=short "${@:2}"
    ;;
  *)
    echo "Usage: ./build.sh {dev|check|test|build|cli|clean|kill|webdriver|e2e|env}"
    exit 1
    ;;
esac
