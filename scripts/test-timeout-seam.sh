#!/usr/bin/env bash
# Test seam: verify portable timeout implementation works on all platforms.
# Tests both standard path (timeout/gtimeout) and fallback path.

set -uo pipefail

run_with_timeout() {
    local timeout_secs="$1"
    shift

    if command -v timeout >/dev/null 2>&1; then
        timeout --kill-after=5s "${timeout_secs}s" "$@"
        return $?
    fi

    if command -v gtimeout >/dev/null 2>&1; then
        gtimeout --kill-after=5s "${timeout_secs}s" "$@"
        return $?
    fi

    "$@" &
    local pid=$!
    local deadline=$(($(date +%s) + timeout_secs))

    while kill -0 "$pid" 2>/dev/null; do
        if [ "$(date +%s)" -ge "$deadline" ]; then
            kill -TERM "$pid" 2>/dev/null
            sleep 1
            kill -KILL "$pid" 2>/dev/null
            wait "$pid" 2>/dev/null
            return 124
        fi
        sleep 0.1
    done

    wait "$pid"
    return $?
}

test_count=0
passed=0

run_test() {
    local name="$1"
    local expected_exit="$2"
    shift 2
    test_count=$((test_count + 1))

    if "$@"; then
        actual_exit=0
    else
        actual_exit=$?
    fi

    if [ "$actual_exit" -eq "$expected_exit" ]; then
        echo "✓ $name (exit code $actual_exit)"
        passed=$((passed + 1))
    else
        echo "✗ $name: expected exit $expected_exit, got $actual_exit"
    fi
}

echo "Testing portable timeout implementation..."
echo

echo "1. Command succeeds before timeout:"
run_test "fast command exits 0" 0 run_with_timeout 10 /bin/true

echo
echo "2. Command exceeds timeout:"
run_test "slow command times out" 124 run_with_timeout 1 /bin/bash -c 'sleep 5'

echo
echo "3. Command fails before timeout:"
run_test "failed command exits non-zero" 1 run_with_timeout 10 /bin/false

echo
echo "4. Timeout with immediate failure:"
run_test "quick failure" 42 run_with_timeout 10 /bin/bash -c 'exit 42'

echo
if command -v timeout >/dev/null 2>&1 || command -v gtimeout >/dev/null 2>&1; then
    echo "ℹ️  Standard timeout utility found ($(command -v timeout || command -v gtimeout))"
else
    echo "ℹ️  Using fallback timeout implementation (no timeout/gtimeout available)"
fi

echo
echo "════════════════════════════════════════════"
echo "Results: $passed/$test_count tests passed"
if [ "$passed" -eq "$test_count" ]; then
    echo "✓ All tests passed"
    exit 0
else
    echo "✗ Some tests failed"
    exit 1
fi
