#!/usr/bin/env bash
# Run every TS unit test under src/, packages/, and selected harness helpers.
#
# These are self-contained tsx scripts (each owns its assert harness and exits
# non-zero on failure). Plugin-side tests have their own runner
# (scripts/plugin-tests.sh, exposed as `pnpm test:plugins`) and are NOT included
# here. Invoked as `npm run test:unit` and folded into `./build.sh test:all`.

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

jobs="${TS_UNIT_JOBS:-4}"
test_timeout="${TS_UNIT_TIMEOUT_SECONDS:-60}"
suite_timeout="${TS_UNIT_SUITE_TIMEOUT_SECONDS:-80}"
if ! [[ "$jobs" =~ ^[1-9][0-9]*$ &&
    "$test_timeout" =~ ^[1-9][0-9]*$ &&
    "$suite_timeout" =~ ^[1-9][0-9]*$ ]]; then
    echo "TS_UNIT_JOBS, TS_UNIT_TIMEOUT_SECONDS, and TS_UNIT_SUITE_TIMEOUT_SECONDS must be positive integers" >&2
    exit 2
fi

tests=()
if [ -n "${CODETERM_TS_UNIT_RUNNER_TEST_LIST:-}" ]; then
    while IFS= read -r f || [ -n "$f" ]; do
        [ -n "$f" ] && tests+=("$f")
    done <"$CODETERM_TS_UNIT_RUNNER_TEST_LIST"
else
    while IFS= read -r f; do
        tests+=("$f")
    done < <(
        find src packages -type f \( -name '*.test.ts' -o -name '*.test.tsx' \) \
            | grep -Ev '/node_modules/|/dist/' \
            | sort
    )
fi

if [ "${#tests[@]}" -eq 0 ]; then
    echo "no TS unit tests found under src/ or packages/"
    exit 0
fi

tsx_cli="$ROOT/node_modules/tsx/dist/cli.mjs"
if [ ! -f "$tsx_cli" ]; then
    echo "TS unit runner: local tsx CLI is missing at $tsx_cli" >&2
    echo "Install workspace dependencies before running unit tests." >&2
    exit 2
fi

if ! command -v perl >/dev/null 2>&1; then
    echo "TS unit runner: Perl is required for process-tree supervision" >&2
    exit 2
fi

timeout_mode="portable"
timeout_program=""
if [ -z "${CODETERM_FORCE_PORTABLE_TIMEOUT:-}" ]; then
    if command -v timeout >/dev/null 2>&1; then
        timeout_mode="gnu"
        timeout_program="$(command -v timeout)"
    elif command -v gtimeout >/dev/null 2>&1; then
        timeout_mode="gnu"
        timeout_program="$(command -v gtimeout)"
    fi
fi

log_dir="$(mktemp -d)"
completion_fifo="$log_dir/completions"
timing_file="$log_dir/timings.tsv"
mkfifo "$completion_fifo"
exec 9<>"$completion_fifo"

worker_pids=()
child_pids=()
states=()
statuses=()
durations_ms=()
cpu_ms=()
started_at=()
log_files=()
active=0
fails=0
passed=0
interrupt_signal=""
is_windows=0
case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) is_windows=1 ;;
esac

for i in "${!tests[@]}"; do
    worker_pids[$i]=""
    child_pids[$i]=""
    states[$i]="pending"
    statuses[$i]=""
    durations_ms[$i]=""
    cpu_ms[$i]=""
    started_at[$i]=""
    log_files[$i]=""
done

trace_schedule() {
    [ -n "${TS_UNIT_TRACE:-}" ] || return
    printf '[ts-unit trace] %s\n' "$*"
}

force_kill_worker() {
    local index="$1"
    local worker="${worker_pids[$index]}"
    local child="${child_pids[$index]}"

    if [ "$is_windows" -eq 1 ]; then
        [ -z "$worker" ] || taskkill.exe //PID "$worker" //T //F >/dev/null 2>&1 || true
        [ -z "$child" ] || taskkill.exe //PID "$child" //T //F >/dev/null 2>&1 || true
    else
        [ -z "$child" ] || kill -KILL -- "-$child" 2>/dev/null || true
        [ -z "$worker" ] || kill -KILL "$worker" 2>/dev/null || true
    fi

    [ -z "$worker" ] || wait "$worker" 2>/dev/null || true
}

cleanup() {
    local index
    for index in "${!tests[@]}"; do
        if [ "${states[$index]}" = "running" ]; then
            force_kill_worker "$index"
        fi
    done
    exec 9>&-
    rm -rf "$log_dir"
}

trap cleanup EXIT
trap 'interrupt_signal=TERM' TERM
trap 'interrupt_signal=INT' INT

run_test_worker() {
    local index="$1"
    shift

    exec perl -e '
    use POSIX qw(setpgid);
    use Time::HiRes qw(time);
    use strict;
    use warnings;

    my ($index, $timeout, $mode, $completion_fd, $timeout_program, @command) = @ARGV;
    open my $completion, ">&=$completion_fd"
        or die "cannot open completion fd $completion_fd: $!";

    my $started = time();
    my @cpu_started = times();
    my $child;
    my $timed_out = 0;
    my $stop_requested = 0;
    my $alarm_stage = 0;

    local $SIG{TERM} = sub {
        $stop_requested = 1;
        kill 9, -$child if $child;
    };
    local $SIG{INT} = $SIG{TERM};
    local $SIG{ALRM} = sub {
        $alarm_stage++;
        if ($alarm_stage == 1) {
            $timed_out = 1;
            kill 15, -$child if $child;
            alarm 5;
        } else {
            kill 9, -$child if $child;
            alarm 0;
        }
    };

    $child = fork();
    exit 1 unless defined $child;

    if ($child == 0) {
        setpgid(0, 0);
        if ($mode eq "gnu") {
            exec { $timeout_program } $timeout_program,
                "--signal=TERM", "--kill-after=5s", "${timeout}s", @command;
        } else {
            exec { $command[0] } @command;
        }
        exit 127;
    }

    setpgid($child, $child);
    print {$completion} "start\t$index\t$child\t0\n";
    alarm $timeout if $mode eq "portable";

    my $waited;
    do {
        $waited = waitpid($child, 0);
    } while ($waited == -1 && $!{EINTR});
    my $raw_status = $?;
    alarm 0;

    my $status = $raw_status >> 8;
    $status = 128 + ($raw_status & 127) if $raw_status & 127;
    $status = 124 if $timed_out;
    $status = 143 if $stop_requested;

    if ($status == 124 || $stop_requested) {
        kill 9, -$child;
    }

    my $elapsed_ms = int((time() - $started) * 1000 + 0.5);
    my @cpu_finished = times();
    my $cpu_ms = int(
        ($cpu_finished[2] + $cpu_finished[3] -
            $cpu_started[2] - $cpu_started[3]) * 1000 + 0.5
    );
    print {$completion} "done\t$index\t$status\t$elapsed_ms\t$cpu_ms\n";
    close $completion;
    exit $status;
    ' "$index" "$test_timeout" "$timeout_mode" 9 "$timeout_program" "$@"
}

launch_test() {
    local index="$1"
    local test_name="${tests[$index]}"
    local log="$log_dir/$index.log"

    states[$index]="running"
    started_at[$index]="$SECONDS"
    log_files[$index]="$log"
    run_test_worker "$index" node "$tsx_cli" "$test_name" >"$log" 2>&1 &
    worker_pids[$index]=$!
    active=$((active + 1))
    trace_schedule "launched index=$index active=$active file=$test_name"
}

finish_test() {
    local index="$1"
    local status="$2"
    local elapsed_ms="$3"
    local consumed_cpu_ms="$4"
    local worker="${worker_pids[$index]}"
    local test_name="${tests[$index]}"
    local log="${log_files[$index]}"

    wait "$worker" 2>/dev/null || true
    statuses[$index]="$status"
    durations_ms[$index]="$elapsed_ms"
    cpu_ms[$index]="$consumed_cpu_ms"
    printf '%s\t%s\t%s\n' "$elapsed_ms" "$consumed_cpu_ms" "$test_name" >>"$timing_file"

    echo
    echo "════ $test_name (${elapsed_ms}ms) ════"
    cat "$log"
    if [ "$status" -eq 0 ]; then
        states[$index]="passed"
        passed=$((passed + 1))
    else
        states[$index]="failed"
        fails=$((fails + 1))
        if [ "$status" -eq 124 ]; then
            echo "  ✗ TIMED OUT after ${test_timeout}s: $test_name"
        else
            echo "  ✗ FAILED: $test_name"
        fi
    fi

    active=$((active - 1))
    trace_schedule "completed index=$index status=$status active=$active file=$test_name"
}

wait_for_completion_until() {
    local deadline="$1"
    local remaining event index value extra detail

    while [ "$active" -gt 0 ]; do
        [ -z "$interrupt_signal" ] || return 1
        remaining=$((deadline - SECONDS))
        [ "$remaining" -gt 0 ] || return 1

        if ! IFS=$'\t' read -r -t "$remaining" event index value extra detail <&9; then
            return 1
        fi

        case "$event" in
            start)
                child_pids[$index]="$value"
                trace_schedule "started index=$index child=$value file=${tests[$index]}"
                ;;
            done)
                finish_test "$index" "$value" "$extra" "$detail"
                return 0
                ;;
            *)
                echo "TS unit runner: invalid completion event: $event" >&2
                return 1
                ;;
        esac
    done
    return 0
}

print_timing_summary() {
    local completed=$((passed + fails))
    local total_ms=0
    local total_cpu_ms=0
    local floor_ms=0
    local elapsed=$((SECONDS - suite_started))
    local duration consumed_cpu

    if [ -s "$timing_file" ]; then
        while IFS=$'\t' read -r duration consumed_cpu _; do
            total_ms=$((total_ms + duration))
            total_cpu_ms=$((total_cpu_ms + consumed_cpu))
        done <"$timing_file"
        floor_ms=$(((total_ms + jobs - 1) / jobs))
    fi

    echo
    echo "TS unit timing: $completed completed file(s), TS_UNIT_JOBS=$jobs"
    echo "Aggregate per-file wall time: ${total_ms}ms"
    echo "Total child CPU time: ${total_cpu_ms}ms"
    echo "Width-${jobs} serialized floor: ceil(${total_ms}ms / $jobs) = ${floor_ms}ms"
    echo "Observed suite wall time: ${elapsed}s"
    if [ -s "$timing_file" ]; then
        echo "Per-file wall time, slowest first:"
        sort -nr -k1,1 "$timing_file" | while IFS=$'\t' read -r duration consumed_cpu test_name; do
            printf '  %8sms wall  %8sms CPU  %s\n' "$duration" "$consumed_cpu" "$test_name"
        done
    fi
}

print_deadline_summary() {
    local reason="$1"
    local deadline_elapsed="$2"
    local passed_list="$3"
    local failed_list="$4"
    local running_list="$5"
    local pending_list="$6"
    local passed_indices=()
    local failed_indices=()
    local running_indices=()
    local pending_indices=()
    local index

    [ -z "$passed_list" ] || read -r -a passed_indices <<<"$passed_list"
    [ -z "$failed_list" ] || read -r -a failed_indices <<<"$failed_list"
    [ -z "$running_list" ] || read -r -a running_indices <<<"$running_list"
    [ -z "$pending_list" ] || read -r -a pending_indices <<<"$pending_list"

    echo
    echo "════════ TS UNIT SUITE INCOMPLETE ════════"
    echo "$reason"

    echo "PASSED AT DEADLINE (${#passed_indices[@]}):"
    for index in "${passed_indices[@]}"; do
        echo "  ✓ ${tests[$index]} (${durations_ms[$index]}ms)"
    done

    echo "FAILED AT DEADLINE (${#failed_indices[@]}):"
    for index in "${failed_indices[@]}"; do
        echo "  ✗ ${tests[$index]} (exit ${statuses[$index]}, ${durations_ms[$index]}ms)"
    done

    echo "STILL RUNNING AT DEADLINE (${#running_indices[@]}):"
    for index in "${running_indices[@]}"; do
        echo "  … ${tests[$index]} (${deadline_elapsed}s suite elapsed)"
    done

    echo "NEVER STARTED (${#pending_indices[@]}):"
    for index in "${pending_indices[@]}"; do
        echo "  - ${tests[$index]}"
    done
    echo "✗ TS unit suite did not complete"
}

stop_after_deadline() {
    local reason="$1"
    local deadline_elapsed=$((SECONDS - suite_started))
    local passed_indices=()
    local failed_indices=()
    local running_indices=()
    local pending_indices=()
    local index cleanup_deadline

    for index in "${!tests[@]}"; do
        case "${states[$index]}" in
            passed) passed_indices+=("$index") ;;
            failed) failed_indices+=("$index") ;;
            running) running_indices+=("$index") ;;
            pending) pending_indices+=("$index") ;;
        esac
    done

    echo
    echo "TS unit runner: $reason; terminating ${#running_indices[@]} running worker(s)" >&2
    for index in "${running_indices[@]}"; do
        kill -TERM "${worker_pids[$index]}" 2>/dev/null || true
    done

    cleanup_deadline=$((SECONDS + 5))
    while [ "$active" -gt 0 ]; do
        if ! wait_for_completion_until "$cleanup_deadline"; then
            break
        fi
    done

    for index in "${running_indices[@]}"; do
        if [ "${states[$index]}" = "running" ]; then
            force_kill_worker "$index"
            states[$index]="stopped"
            active=$((active - 1))
        fi
    done

    print_timing_summary
    print_deadline_summary \
        "$reason" \
        "$deadline_elapsed" \
        "${passed_indices[*]}" \
        "${failed_indices[*]}" \
        "${running_indices[*]}" \
        "${pending_indices[*]}"
    exit 1
}

suite_started=$SECONDS
suite_deadline=$((suite_started + suite_timeout))
next_index=0
test_count=${#tests[@]}

while [ "$next_index" -lt "$test_count" ]; do
    [ -z "$interrupt_signal" ] || stop_after_deadline "received $interrupt_signal"
    if [ "$SECONDS" -ge "$suite_deadline" ]; then
        stop_after_deadline "global deadline reached after ${suite_timeout}s"
    fi

    while [ "$active" -lt "$jobs" ] && [ "$next_index" -lt "$test_count" ]; do
        launch_test "$next_index"
        next_index=$((next_index + 1))
    done

    if ! wait_for_completion_until "$suite_deadline"; then
        if [ -n "$interrupt_signal" ]; then
            stop_after_deadline "received $interrupt_signal"
        else
            stop_after_deadline "global deadline reached after ${suite_timeout}s"
        fi
    fi
done

while [ "$active" -gt 0 ]; do
    if ! wait_for_completion_until "$suite_deadline"; then
        if [ -n "$interrupt_signal" ]; then
            stop_after_deadline "received $interrupt_signal"
        else
            stop_after_deadline "global deadline reached after ${suite_timeout}s"
        fi
    fi
done

print_timing_summary
echo
if [ "$fails" -eq 0 ]; then
    echo "✓ all ${#tests[@]} TS unit test file(s) green"
    exit 0
else
    echo "✗ $fails of ${#tests[@]} TS unit test file(s) failed"
    exit 1
fi
