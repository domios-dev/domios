#!/usr/bin/env bash
# CodeTerm build script — works on macOS, Linux, and Windows (BLACKSTAR).
# Usage:
#   ./build.sh dev       — dev mode (hot reload, incremental)
#   ./build.sh check     — cargo check only (~2s incremental)
#   ./build.sh test      — run Rust unit tests
#   ./build.sh build     — production build
#   ./build.sh cli       — build CLI and install to ~/.codeterm/bin/
#   ./build.sh clean     — clean build cache
#   ./build.sh kill      — kill dev instance (safe, won't touch prod)
#   ./build.sh webdriver — dev with WebDriver for UI tests
#   ./build.sh e2e       — run E2E tests against dev (port 7686)
#   source build.sh env  — load MSVC env vars into current shell (Windows only)

set -e

# --- MSVC environment (Windows only) ---
setup_env() {
  if [[ "$OSTYPE" != darwin* ]] && [[ "$OSTYPE" != linux* ]]; then
    MSVC_VER="14.44.35207"
    BT_BASE="C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools"
    SDK_VER="10.0.22621.0"
    SDK_BASE="C:/Program Files (x86)/Windows Kits/10"
    export LIB="$BT_BASE/VC/Tools/MSVC/$MSVC_VER/lib/x64;$SDK_BASE/Lib/$SDK_VER/ucrt/x64;$SDK_BASE/Lib/$SDK_VER/um/x64"
    export INCLUDE="$BT_BASE/VC/Tools/MSVC/$MSVC_VER/include;$SDK_BASE/Include/$SDK_VER/ucrt;$SDK_BASE/Include/$SDK_VER/um;$SDK_BASE/Include/$SDK_VER/shared"
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
    pnpm tauri build
    ;;
  cli)
    touch cli/src/main.rs
    cargo build --release --manifest-path cli/Cargo.toml
    BIN_DIR="$HOME/.codeterm/bin"
    mkdir -p "$BIN_DIR"
    if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
      cp cli/target/release/codeterm "$BIN_DIR/codeterm"
    else
      cp cli/target/release/codeterm.exe "$BIN_DIR/codeterm.exe"
    fi
    echo "CLI installed to $BIN_DIR"
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
    echo "Starting dev..."
    pnpm tauri dev --config src-tauri/tauri.dev.conf.json
    ;;
  webdriver)
    kill_dev
    echo "Starting dev with WebDriver..."
    pnpm tauri dev --config src-tauri/tauri.dev.conf.json -- --features webdriver
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
