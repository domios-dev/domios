#!/usr/bin/env bash
# Run every TS unit test under src/, packages/, and selected harness helpers.
#
# These are self-contained tsx scripts (each owns its assert harness and exits
# non-zero on failure). Plugin-side tests have their own runner
# (scripts/plugin-tests.sh, exposed as `pnpm test:plugins`) and are NOT included
# here. Invoked as `pnpm test:unit` and folded into `./build.sh test:all`.

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

jobs="${TS_UNIT_JOBS:-4}"
test_timeout="${TS_UNIT_TIMEOUT_SECONDS:-60}"
if ! [[ "$jobs" =~ ^[1-9][0-9]*$ && "$test_timeout" =~ ^[1-9][0-9]*$ ]]; then
    echo "TS_UNIT_JOBS and TS_UNIT_TIMEOUT_SECONDS must be positive integers" >&2
    exit 2
fi

tests=()
while IFS= read -r f; do
    tests+=("$f")
done < <(
    find src packages -type f \( -name '*.test.ts' -o -name '*.test.tsx' \) \
        | grep -Ev '/node_modules/|/dist/' \
        | sort
)

if [ "${#tests[@]}" -eq 0 ]; then
    echo "no TS unit tests found under src/ or packages/"
    exit 0
fi

log_dir="$(mktemp -d)"
trap 'rm -rf "$log_dir"' EXIT

pids=()
test_names=()
log_files=()
fails=0

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

    local timeout_sentinel
    timeout_sentinel="$(mktemp)" || return 1
    trap "rm -f '$timeout_sentinel'" RETURN

    "$@" &
    local pid=$!

    (sleep "$timeout_secs" && touch "$timeout_sentinel" && kill -TERM -"$pid" 2>/dev/null; sleep 5; kill -KILL -"$pid" 2>/dev/null) &
    local monitor_pid=$!

    wait "$pid" 2>/dev/null
    local status=$?

    kill "$monitor_pid" 2>/dev/null
    wait "$monitor_pid" 2>/dev/null

    if [ -f "$timeout_sentinel" ]; then
        return 124
    fi

    return $status
}

reap_one() {
    local i status t log

    if [ ${#pids[@]} -eq 0 ]; then
        return
    fi

    if wait "${pids[0]}"; then
        status=0
    else
        status=$?
    fi

    t="${test_names[0]}"
    log="${log_files[0]}"
    echo
    echo "════ $t ════"
    cat "$log"
    if [ "$status" -ne 0 ]; then
        fails=$((fails + 1))
        if [ "$status" -eq 124 ]; then
            echo "  ✗ TIMED OUT after ${test_timeout}s: $t"
        else
            echo "  ✗ FAILED: $t"
        fi
    fi

    pids=("${pids[@]:1}")
    test_names=("${test_names[@]:1}")
    log_files=("${log_files[@]:1}")
}

for i in "${!tests[@]}"; do
    t="${tests[$i]}"
    log="$log_dir/$i.log"
    run_with_timeout "$test_timeout" npx tsx "$t" >"$log" 2>&1 &
    pid=$!
    pids+=("$pid")
    test_names+=("$t")
    log_files+=("$log")
    if [ "${#pids[@]}" -ge "$jobs" ]; then
        reap_one
    fi
done

while [ "${#pids[@]}" -gt 0 ]; do
    reap_one
done

echo
if [ "$fails" -eq 0 ]; then
    echo "✓ all ${#tests[@]} TS unit test file(s) green"
    exit 0
else
    echo "✗ $fails of ${#tests[@]} TS unit test file(s) failed"
    exit 1
fi
