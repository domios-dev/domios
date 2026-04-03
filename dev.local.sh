#!/usr/bin/env bash
# Local dev helper for BLACKSTAR (Windows, VS 2022 BuildTools)
# This file is gitignored (*.local.sh)
#
# Usage:
#   source dev.local.sh env     # load MSVC env vars into current shell
#   bash dev.local.sh dev       # start dev server (background)
#   bash dev.local.sh test      # run e2e tests (needs running dev server)
#   bash dev.local.sh all       # dev + wait + test

set -euo pipefail

MSVC_VER="14.44.35207"
BT_BASE="C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools"
SDK_VER="10.0.22621.0"
SDK_BASE="C:/Program Files (x86)/Windows Kits/10"
MSVC_BIN="$BT_BASE/VC/Tools/MSVC/$MSVC_VER/bin/HostX64/x64"

setup_msvc_env() {
    export LIB="$BT_BASE/VC/Tools/MSVC/$MSVC_VER/lib/x64;$SDK_BASE/Lib/$SDK_VER/ucrt/x64;$SDK_BASE/Lib/$SDK_VER/um/x64"
    export INCLUDE="$BT_BASE/VC/Tools/MSVC/$MSVC_VER/include;$SDK_BASE/Include/$SDK_VER/ucrt;$SDK_BASE/Include/$SDK_VER/um;$SDK_BASE/Include/$SDK_VER/shared"
    export CC="$MSVC_BIN/cl.exe"
    export CXX="$MSVC_BIN/cl.exe"
    export PATH="$MSVC_BIN:$PATH"
    echo "MSVC env loaded (VS 2022 BuildTools, MSVC $MSVC_VER)"
}

start_dev() {
    setup_msvc_env
    # Kill previous dev instance (match "codeterm-dev" identifier, spare prod)
    local dev_port=7686
    local dev_pid
    dev_pid=$(netstat -ano 2>/dev/null | grep ":${dev_port}.*LISTEN" | awk '{print $5}' | head -1)
    if [ -n "$dev_pid" ] && [ "$dev_pid" != "0" ]; then
        echo "Killing previous dev (PID $dev_pid on port $dev_port)..."
        taskkill //PID "$dev_pid" //F 2>/dev/null || true
        sleep 2
    fi
    echo "Starting dev server..."
    pnpm tauri dev --config src-tauri/tauri.dev.conf.json > /tmp/tauri-dev.log 2>&1 &
    echo "Dev PID: $!"
}

wait_for_api() {
    local logfile="$USERPROFILE/.codeterm-dev/logs/codeterm.log"
    echo "Waiting for REST API..."
    for i in $(seq 1 90); do
        if [ -f "$logfile" ] && grep -q "REST API listening" "$logfile" 2>/dev/null; then
            echo "REST API is up"
            return 0
        fi
        sleep 2
    done
    echo "TIMEOUT waiting for REST API"
    return 1
}

run_tests() {
    export CODETERM_API_URL="http://127.0.0.1:7686"
    python -m pytest tests/e2e/ -v "$@"
}

case "${1:-help}" in
    env)
        setup_msvc_env
        ;;
    dev)
        start_dev
        wait_for_api
        ;;
    test)
        run_tests "${@:2}"
        ;;
    all)
        start_dev
        wait_for_api
        run_tests "${@:2}"
        ;;
    *)
        echo "Usage: dev.local.sh {env|dev|test|all}"
        echo "  env  — load MSVC env (use: source dev.local.sh env)"
        echo "  dev  — start dev server + wait for API"
        echo "  test — run e2e tests"
        echo "  all  — dev + test"
        ;;
esac
