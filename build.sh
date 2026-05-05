#!/usr/bin/env bash
# CodeTerm build script — works on macOS, Linux, and Windows.
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

# --- Load .env (build secrets like UPDATER_TOKEN) ---
if [ -f ".env" ]; then
  set -a
  source .env
  set +a
fi

# Auto-discover VS edition + MSVC + SDK. Override via VS_EDITION / MSVC_VER / SDK_VER env.
setup_env() {
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    return 0
  fi

  local NODE_DIR
  NODE_DIR=$(ls -d "$(pwd)/.tools/node-v"*-win-x64 2>/dev/null | sort -V | tail -n 1)
  if [ -n "$NODE_DIR" ] && [ -x "$NODE_DIR/node.exe" ]; then
    local NODE_BASH="${NODE_DIR/C:/\/c}"
    NODE_BASH="${NODE_BASH//\\/\/}"
    export PATH="$NODE_BASH:$PATH"
  fi

  local SDK_BASE="C:/Program Files (x86)/Windows Kits/10"

  local VS_EDITION="${VS_EDITION:-}"
  local BT_BASE=""
  if [ -n "$VS_EDITION" ]; then
    if [ -d "C:/Program Files (x86)/Microsoft Visual Studio/2022/$VS_EDITION" ]; then
      BT_BASE="C:/Program Files (x86)/Microsoft Visual Studio/2022/$VS_EDITION"
    elif [ -d "C:/Program Files/Microsoft Visual Studio/2022/$VS_EDITION" ]; then
      BT_BASE="C:/Program Files/Microsoft Visual Studio/2022/$VS_EDITION"
    fi
  else
    for ed in BuildTools Community Professional Enterprise; do
      if [ -d "C:/Program Files (x86)/Microsoft Visual Studio/2022/$ed" ]; then
        BT_BASE="C:/Program Files (x86)/Microsoft Visual Studio/2022/$ed"
        VS_EDITION="$ed"
        break
      fi
      if [ -d "C:/Program Files/Microsoft Visual Studio/2022/$ed" ]; then
        BT_BASE="C:/Program Files/Microsoft Visual Studio/2022/$ed"
        VS_EDITION="$ed"
        break
      fi
    done
  fi
  if [ -z "$BT_BASE" ]; then
    echo "Error: Visual Studio 2022 not found. Install BuildTools / Community / Professional / Enterprise." >&2
    return 1
  fi

  local MSVC_VER="${MSVC_VER:-}"
  if [ -z "$MSVC_VER" ]; then
    MSVC_VER=$(ls "$BT_BASE/VC/Tools/MSVC/" 2>/dev/null | sort -V | tail -n 1)
  fi
  if [ -z "$MSVC_VER" ] || [ ! -d "$BT_BASE/VC/Tools/MSVC/$MSVC_VER" ]; then
    echo "Error: no MSVC toolchain under $BT_BASE/VC/Tools/MSVC/" >&2
    return 1
  fi

  local SDK_VER="${SDK_VER:-}"
  if [ -z "$SDK_VER" ]; then
    SDK_VER=$(ls "$SDK_BASE/Include/" 2>/dev/null | grep -E '^10\.' | sort -V | tail -n 1)
  fi
  if [ -z "$SDK_VER" ] || [ ! -d "$SDK_BASE/Include/$SDK_VER" ]; then
    echo "Error: no Windows SDK under $SDK_BASE/Include/" >&2
    return 1
  fi

  export CC="$BT_BASE/VC/Tools/MSVC/$MSVC_VER/bin/Hostx64/x64/cl.exe"
  export CXX="$BT_BASE/VC/Tools/MSVC/$MSVC_VER/bin/Hostx64/x64/cl.exe"
  export LIB="$BT_BASE/VC/Tools/MSVC/$MSVC_VER/lib/x64;$SDK_BASE/Lib/$SDK_VER/ucrt/x64;$SDK_BASE/Lib/$SDK_VER/um/x64"
  export INCLUDE="$BT_BASE/VC/Tools/MSVC/$MSVC_VER/include;$SDK_BASE/Include/$SDK_VER/ucrt;$SDK_BASE/Include/$SDK_VER/um;$SDK_BASE/Include/$SDK_VER/shared"
  export RC="$SDK_BASE/bin/$SDK_VER/x64/rc.exe"

  local BT_BASH="${BT_BASE/C:/\/c}"
  export PATH="${BT_BASH}/VC/Tools/MSVC/$MSVC_VER/bin/HostX64/x64:/c/Program Files (x86)/Windows Kits/10/bin/$SDK_VER/x64:$PATH"
  echo "MSVC env loaded (VS 2022 $VS_EDITION, MSVC $MSVC_VER, SDK $SDK_VER)"
}

# Kill dev codeterm + vite by binary path. Prod lives elsewhere — never matched.
kill_dev() {
  if [[ "$OSTYPE" == darwin* ]]; then
    pgrep -f "target/debug/codeterm" | xargs kill 2>/dev/null || true
    lsof -ti:1420 | xargs kill -9 2>/dev/null || true
  else
    local pids
    pids=$(powershell.exe -NoProfile -Command "
      Get-Process codeterm,node -ErrorAction SilentlyContinue |
        Where-Object { \$_.Path -like '*\\target\\debug\\codeterm.exe' -or \$_.Path -like '*\\.tools\\*' } |
        ForEach-Object { \$_.Id }
    " 2>/dev/null | tr -d '\r')
    if [ -n "$pids" ]; then
      for pid in $pids; do taskkill //PID "$pid" //F 2>/dev/null || true; done
    fi
    sleep 1
  fi
}

setup_env

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
    ensure_deps
    # Increment build number
    BUILD_NUM=$(cat BUILD_NUMBER 2>/dev/null || echo 0)
    BUILD_NUM=$((BUILD_NUM + 1))
    echo "$BUILD_NUM" > BUILD_NUMBER
    # Build CLI first (so it's ready when app starts)
    echo "Building CLI..."
    cargo build --release --manifest-path cli/Cargo.toml
    BIN_DIR="$HOME/.codeterm/bin"
    mkdir -p "$BIN_DIR"
    if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
      cp cli/target/release/codeterm "$BIN_DIR/codeterm"
    else
      cp cli/target/release/codeterm.exe "$BIN_DIR/codeterm.exe"
    fi
    echo "CLI installed. Build #$BUILD_NUM starting..."
    pnpm tauri dev --no-watch --config src-tauri/tauri.dev.conf.json
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
