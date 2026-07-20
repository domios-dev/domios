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

declare -a pids=()
declare -A pid_test=()
declare -A pid_log=()
fails=0

reap_one() {
    local completed_pid status t log remaining_pid
    if wait -n -p completed_pid "${pids[@]}"; then
        status=0
    else
        status=$?
    fi

    t="${pid_test[$completed_pid]}"
    log="${pid_log[$completed_pid]}"
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

    unset 'pid_test[$completed_pid]' 'pid_log[$completed_pid]'
    local -a remaining=()
    for remaining_pid in "${pids[@]}"; do
        if [ "$remaining_pid" != "$completed_pid" ]; then
            remaining+=("$remaining_pid")
        fi
    done
    pids=("${remaining[@]}")
}

for i in "${!tests[@]}"; do
    t="${tests[$i]}"
    log="$log_dir/$i.log"
    timeout --kill-after=5s "${test_timeout}s" npx tsx "$t" >"$log" 2>&1 &
    pid=$!
    pids+=("$pid")
    pid_test["$pid"]="$t"
    pid_log["$pid"]="$log"
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
