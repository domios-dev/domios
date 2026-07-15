#!/usr/bin/env bash

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
WRAPPER="$ROOT/scripts/test-capped.sh"
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

run_prompt_case() {
    local name=$1
    local expected_exit=$2
    local command=$3
    shift 3
    local started=$SECONDS
    local actual_exit=0
    local output="$TMPDIR/$name.out"

    if TEST_CAP_SECS=90 CARGO_TEST_CMD="$command" "$WRAPPER" "$@" >"$output" 2>&1; then
        actual_exit=0
    else
        actual_exit=$?
    fi

    [ "$actual_exit" -eq "$expected_exit" ] || {
        printf '%s: expected exit %s, got %s\n' "$name" "$expected_exit" "$actual_exit" >&2
        return 1
    }
    [ $((SECONDS - started)) -lt 10 ] || {
        printf '%s: exceeded 10s failure deadline\n' "$name" >&2
        return 1
    }
    ! grep -q 'DEADLOCK / HANG' "$output" || {
        printf '%s: timeout banner printed after prompt exit\n' "$name" >&2
        return 1
    }
}

run_prompt_case fast-success 0 true
run_prompt_case fast-failure 3 bash -c 'exit 3'

child_pid_file="$TMPDIR/child.pid"
timeout_exit=0
timeout_output="$TMPDIR/timeout.out"
if TEST_CAP_SECS=2 CARGO_TEST_CMD=bash "$WRAPPER" -c '
    sleep 300 &
    child_pid=$!
    printf "%s" "$child_pid" > "$1"
    wait "$child_pid"
' bash "$child_pid_file" >"$timeout_output" 2>&1; then
    timeout_exit=0
else
    timeout_exit=$?
fi

[ "$timeout_exit" -eq 124 ] || {
    printf 'timeout: expected exit 124, got %s\n' "$timeout_exit" >&2
    exit 1
}
[ "$(grep -c 'DEADLOCK / HANG' "$timeout_output")" -eq 1 ] || {
    printf 'timeout: expected one timeout banner\n' >&2
    exit 1
}
[ -s "$child_pid_file" ] || {
    printf 'timeout: child PID was not recorded\n' >&2
    exit 1
}
child_pid=$(<"$child_pid_file")
if kill -0 "$child_pid" 2>/dev/null; then
    printf 'timeout: child process %s survived\n' "$child_pid" >&2
    exit 1
fi
