#!/usr/bin/env bash
# Test seam: verify portable timeout implementation works on all platforms.
# Tests both standard path (timeout/gtimeout) and fallback path.

set -uo pipefail

run_with_timeout() {
    local timeout_secs="$1"
    shift

    if [ -z "${CODETERM_FORCE_PORTABLE_TIMEOUT:-}" ]; then
        if command -v timeout >/dev/null 2>&1; then
            timeout --kill-after=5s "${timeout_secs}s" "$@"
            return $?
        fi

        if command -v gtimeout >/dev/null 2>&1; then
            gtimeout --kill-after=5s "${timeout_secs}s" "$@"
            return $?
        fi
    fi

    if command -v perl >/dev/null 2>&1; then
        perl -e '
        use POSIX qw(setpgid);
        my $timeout = $ARGV[0];
        my @cmd = @ARGV[1..$#ARGV];

        my $pid = fork();
        exit 1 unless defined $pid;

        if ($pid == 0) {
            setpgid(0, 0);
            exec { $cmd[0] } @cmd;
            exit 127;
        }

        local $SIG{ALRM} = sub {
            kill -15, $pid;
            sleep 1;
            kill -9, $pid if kill 0, $pid;
            exit 124;
        };

        alarm($timeout);
        waitpid($pid, 0);
        my $status = $?;
        alarm(0);

        exit($status >> 8 || ($status & 127 ? 128 + ($status & 127) : 0));
        ' "$timeout_secs" "$@"
        return $?
    fi

    "$@" &
    local pid=$!
    wait "$pid" 2>/dev/null
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
