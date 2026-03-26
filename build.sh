#!/usr/bin/env bash
# CodeTerm build script for BLACKSTAR (Windows + VS 2022 BuildTools)
# Usage:
#   ./build.sh dev      — dev mode (hot reload)
#   ./build.sh check    — cargo check only
#   ./build.sh test     — run all tests
#   ./build.sh build    — production build
#   ./build.sh cli      — build CLI tool
#   ./build.sh kill     — kill running CodeTerm processes
#   ./build.sh e2e      — run E2E tests (pytest, needs running instance or auto-starts)

set -e

# VS 2022 BuildTools environment (BLACKSTAR has broken VS 18 Insiders)
export LIB="C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/MSVC/14.44.35207/lib/x64;C:/Program Files (x86)/Windows Kits/NETFXSDK/4.8/lib/um/x64;C:/Program Files (x86)/Windows Kits/10/lib/10.0.22621.0/ucrt/x64;C:/Program Files (x86)/Windows Kits/10/lib/10.0.22621.0/um/x64"
export INCLUDE="C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/MSVC/14.44.35207/include;C:/Program Files (x86)/Windows Kits/10/include/10.0.22621.0/ucrt;C:/Program Files (x86)/Windows Kits/10/include/10.0.22621.0/um;C:/Program Files (x86)/Windows Kits/10/include/10.0.22621.0/shared;C:/Program Files (x86)/Windows Kits/10/include/10.0.22621.0/winrt;C:/Program Files (x86)/Windows Kits/10/include/10.0.22621.0/cppwinrt"
export PATH="/c/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/MSVC/14.44.35207/bin/HostX64/x64:$PATH"

CMD="${1:-dev}"

case "$CMD" in
  kill)
    echo "Killing CodeTerm processes..."
    taskkill //F //IM codeterm.exe 2>/dev/null || true
    taskkill //F //IM node.exe 2>/dev/null || true
    echo "Done."
    ;;
  check)
    echo "Checking..."
    cargo check --manifest-path src-tauri/Cargo.toml
    ;;
  test)
    echo "Running tests..."
    cargo test --manifest-path src-tauri/Cargo.toml --lib
    ;;
  build)
    echo "Production build..."
    pnpm tauri build
    ;;
  cli)
    echo "Building CLI..."
    cargo build --release --manifest-path cli/Cargo.toml
    echo "CLI: cli/target/release/codeterm.exe"
    ;;
  dev)
    echo "Starting dev mode..."
    pnpm tauri dev --config src-tauri/tauri.dev.conf.json
    ;;
  e2e)
    echo "Running E2E tests..."
    cd tests/e2e
    pip install -q -r requirements.txt 2>/dev/null
    python3 -m pytest -v --tb=short "$@"
    ;;
  *)
    echo "Usage: ./build.sh {dev|check|test|build|cli|kill|e2e}"
    exit 1
    ;;
esac
