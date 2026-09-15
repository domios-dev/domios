#!/usr/bin/env bash
# Run `cargo test`, optionally under a process-group timeout.
#
# With TEST_CAP_SECS set, kills the WHOLE process group — cargo AND every
# spawned test binary (*/target/*/deps/*) — so no orphans survive the cap.
#
# Uncapped by default. An explicit cap exits 124 on timeout (matches GNU timeout);
# otherwise the wrapper passes through cargo's own exit code.
#
# Fail-closed filtering: when the caller supplies a test-name filter, a run that
# executed zero tests exits 97 instead of reporting cargo's success. A run with
# no filter is never subject to that check.
#
# Usage:
#   scripts/test-capped.sh [-- <cargo-test-args>...]
#   TEST_CAP_SECS=30 scripts/test-capped.sh -- my_crate::my_mod
#
# Overridable env:
#   TEST_CAP_SECS   optional positive seconds before SIGTERM + banner
#   CARGO_TEST_CMD  command to run instead of "cargo test" (for testing this script itself)
#   CODETERM_EMBED_CACHE_DIR shared machine cache for immutable embedding models
#
# Not overridable — always cleared/set before the test command runs:
#   CODETERM_CONPTY_DIR unset unconditionally (an inherited value can point at
#     a real user install; see pty-fork/src/win/psuedocon.rs test isolation)
#   CODETERM_CONPTY_TEST_ISOLATION always exported as 1

set -euo pipefail

TEST_CAP_SECS="${TEST_CAP_SECS:-}"
CARGO_TEST_CMD="${CARGO_TEST_CMD:-cargo test}"
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
# No awk `exit`: it closes the pipe, SIGPIPEs rustc, and pipefail then aborts the
# whole script at exit 141 with zero output.
HOST_TRIPLE="${CODETERM_TEST_HOST_TRIPLE:-$(rustc -vV | awk -F': ' '/^host:/{print $2}')}"

configure_embed_cache() {
    if [ -n "${CODETERM_EMBED_CACHE_DIR:-}" ]; then
        return
    fi
    [ -n "${HOME:-}" ] || {
        printf 'error: HOME is required to select the shared embedding cache.\n' >&2
        exit 2
    }
    CODETERM_EMBED_CACHE_DIR="$HOME/.codeterm/fastembed_cache"
    export CODETERM_EMBED_CACHE_DIR
    printf '[test-capped] shared embed cache: %s\n' "$CODETERM_EMBED_CACHE_DIR"
}

# A test launched from a CodeTerm tab inherits CODETERM_CONPTY_DIR pointing at
# the installed conpty bundle. Always clear it and mark test isolation — never
# rely on it happening to be absent. See pty-fork/src/win/psuedocon.rs.
isolate_conpty_test_dir() {
    # A task-assigned tab exports the plan and task binding, and Clap reads them
    # as defaults, so a parse-with-no-flags test would inherit a binding.
    unset CODETERM_PLAN_ID CODETERM_TASK_ID
    unset CODETERM_CONPTY_DIR
    CODETERM_CONPTY_TEST_ISOLATION=1
    export CODETERM_CONPTY_TEST_ISOLATION
}

[[ -n "$HOST_TRIPLE" ]] || {
    printf 'error: unable to determine Rust host triple.\n' >&2
    exit 2
}

configure_windows_test_target() {
    [[ "$HOST_TRIPLE" == *-windows-* ]] || return 0
    case "${CARGO_TARGET_DIR:-}" in
        *[/\\]CodeTerm[/\\]build-gateway[/\\]targets[/\\]exceptional[/\\]*)
            if [ "${CODETERM_ALLOW_EXTERNAL_TARGET:-0}" = "1" ] &&
                [ "${CARGO_INCREMENTAL:-}" = "0" ] &&
                [ "${RUSTC_WRAPPER:-}" = "sccache" ]; then
                printf '[test-capped] using configured exceptional test target: %s\n' "$CARGO_TARGET_DIR"
            else
                printf '[test-capped] ignoring inherited internal build-gateway target: %s\n' "$CARGO_TARGET_DIR"
                unset CARGO_TARGET_DIR
            fi
            ;;
        *[/\\]CodeTerm[/\\]build-gateway[/\\]targets[/\\]*)
            printf '[test-capped] ignoring inherited internal build-gateway target: %s\n' "$CARGO_TARGET_DIR"
            unset CARGO_TARGET_DIR
            ;;
    esac
    if [ -z "${CARGO_TARGET_DIR:-}" ] &&
        { [ "${CODETERM_TEST_DEV_HEALTH:-}" = "healthy" ] ||
            curl --silent --fail --max-time 1 http://127.0.0.1:7686/api/health >/dev/null 2>&1; }; then
        CARGO_TARGET_DIR="$ROOT/target-check"
        export CARGO_TARGET_DIR
        printf '[test-capped] using isolated Windows test target: %s\n' "$CARGO_TARGET_DIR"
        if command -v sccache >/dev/null 2>&1; then
            RUSTC_WRAPPER=sccache
            export RUSTC_WRAPPER
            # SCCACHE_BASEDIRS strips the worktree path before hashing, which lets a
            # cache hit return an artifact built from a different worktree's sources
            # (codeterm#327). Never set it, and drop it if a parent shell leaked one in.
            unset SCCACHE_BASEDIRS
            printf '[test-capped] sccache enabled for the isolated Windows test target (basedir rewriting disabled; see essences/BUILD.md).\n'
        fi
    fi
    return 0
}

ensure_embed_sidecar() {
    local extension=""
    local sidecar

    [[ "$HOST_TRIPLE" == *-windows-* ]] && extension=".exe"
    sidecar="$ROOT/src-tauri/bundled-embed/codeterm-embed-${HOST_TRIPLE}${extension}"

    [ -e "$sidecar" ] && return

    printf '[test-capped] missing bundled embed sidecar; creating test-only stub.\n'
    (
        cd "$ROOT"
        node scripts/prepare-embed-stub.js >/dev/null
    )
    [ -e "$sidecar" ] || {
        printf 'error: bundled embed stub was not created: %s\n' "$sidecar" >&2
        exit 2
    }
}

ARGS_JOINED=" $* "
if [[ "$ARGS_JOINED" == *" --manifest-path cli/Cargo.toml "* && "$ARGS_JOINED" == *" --lib "* ]]; then
    printf 'error: cli/Cargo.toml is a binary crate and has no --lib test target.\n' >&2
    printf 'hint: use scripts/test-capped.sh --manifest-path cli/Cargo.toml <test-filter>\n' >&2
    exit 2
fi

configure_windows_test_target
configure_embed_cache
isolate_conpty_test_dir
ensure_embed_sidecar

ZERO_MATCH_EXIT=97
REQUESTED_FILTER=""
FILTER_PACKAGE=""
FILTER_MANIFEST=""

# Cargo test syntax is `cargo test [options] [TESTNAME] [-- harness-args]`, so the
# filter is the first positional that no value-taking option already consumed.
detect_requested_filter() {
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --) return 0 ;;
            -p|--package) FILTER_PACKAGE="${2:-}"; shift 2 || return 0 ;;
            --manifest-path) FILTER_MANIFEST="${2:-}"; shift 2 || return 0 ;;
            -p=*|--package=*) FILTER_PACKAGE="${1#*=}"; shift ;;
            --manifest-path=*) FILTER_MANIFEST="${1#*=}"; shift ;;
            --features|-F|--target|--target-dir|--test|--bin|--bench|--example|--exclude|-j|--jobs|--message-format|--profile|--config|-Z|--color)
                shift 2 || return 0 ;;
            -*) shift ;;
            *) REQUESTED_FILTER="$1"; return 0 ;;
        esac
    done
}

# Zero-match only when libtest actually reported, and every reported binary ran
# nothing; a full run's own "running 0 tests" binaries never reach this check.
filtered_run_matched_nothing() {
    local log="$1"
    grep -Eq '^running [0-9]+ tests?[[:space:]]*$' "$log" || return 1
    ! grep -Eq '^running [1-9][0-9]* tests?[[:space:]]*$' "$log"
}

report_zero_match() {
    local context="${FILTER_PACKAGE:-${FILTER_MANIFEST:-unspecified crate}}"
    printf '\n'
    printf '══════════════════════════════════════════════════════════════════\n'
    printf '  ZERO MATCHES: filter %s selected no tests in %s.\n' "$REQUESTED_FILTER" "$context"
    printf '  A filtered run that executed nothing is not a pass.\n'
    printf '  Fix the test name, or drop the filter to run the whole target.\n'
    printf '══════════════════════════════════════════════════════════════════\n'
}

# The cap owns the bounded failure deadline. It preserves the child status unless
# it enforces the deadline, avoiding MSYS job-control group kills.
run_capped() {
    local secs="$1"
    shift
    if command -v timeout >/dev/null 2>&1; then
        timeout --signal=TERM --kill-after=5s "$secs" "$@"
    elif command -v gtimeout >/dev/null 2>&1; then
        gtimeout --signal=TERM --kill-after=5s "$secs" "$@"
    else
        # Stock macOS ships no GNU coreutils; this fallback keeps the same
        # contract: own process group, TERM at the cap, KILL 5s later, exit 124.
        perl -e '
            my $secs = shift @ARGV;
            my $pid = fork();
            die "test-capped: fork failed: $!\n" unless defined $pid;
            if ($pid == 0) { setpgrp(0, 0); exec @ARGV or die "test-capped: exec failed: $!\n"; }
            my $stage = 0;
            $SIG{ALRM} = sub {
                if ($stage == 0) { $stage = 1; kill("TERM", -$pid); alarm(5); }
                else { $stage = 2; kill("KILL", -$pid); alarm(5); }
            };
            alarm($secs);
            my $reaped;
            do { $reaped = waitpid($pid, 0); } while ($reaped < 0 && $! =~ /Interrupted/);
            my $status = $?;
            alarm(0);
            exit(124) if $stage > 0;
            exit($status & 127 ? 128 + ($status & 127) : $status >> 8);
        ' "$secs" "$@"
    fi
}

run_test() {
    if [ -n "$TEST_CAP_SECS" ]; then
        run_capped "$TEST_CAP_SECS" $CARGO_TEST_CMD "$@"
    else
        $CARGO_TEST_CMD "$@"
    fi
}

detect_requested_filter "$@"

if [ -n "$REQUESTED_FILTER" ]; then
    TEST_LOG=$(mktemp)
    trap 'rm -f "$TEST_LOG"' EXIT
    if run_test "$@" | tee "$TEST_LOG"; then
        if filtered_run_matched_nothing "$TEST_LOG"; then
            report_zero_match
            exit "$ZERO_MATCH_EXIT"
        fi
        exit 0
    else
        CARGO_EXIT=$?
    fi
elif run_test "$@"; then
    exit 0
else
    CARGO_EXIT=$?
fi

if [ -n "$TEST_CAP_SECS" ] && [ "$CARGO_EXIT" -eq 124 ]; then
    printf '\n'
    printf '══════════════════════════════════════════════════════════════════\n'
    printf '  DEADLOCK / HANG: cargo test exceeded %ss cap.\n' "$TEST_CAP_SECS"
    printf '  Read the code — do NOT re-run with a bigger cap.\n'
    printf '══════════════════════════════════════════════════════════════════\n'
fi
exit "$CARGO_EXIT"
