#!/usr/bin/env bash
# Caller: build.sh check|check-tests|test-one|release-preflight|install-local|build|test|verify|fmt-check/release-preflight/test-one/verify/test:all; CI qualification.yml; scripts check-build-gateway-policy.sh, check-sccache-basedir-isolation.sh, check-slow-unit-tests.test.mjs; skill build-machine, build-reference, build
# Test duration thresholds are advisory; the command owns its actual exit.
# TEST_CAP_SECS defaults to 600; 0 disables duration warnings.
#
# Fail-closed filtering: when the caller supplies a test-name filter or a nextest
# filterset, a successful run must show a passed test in a libtest or nextest
# summary; otherwise it exits 97. An explicit --no-tests=pass|warn opts out, and
# a run with no filter is never subject to that check.
#
# Usage:
#   scripts/test-capped.sh [<cargo-test-args>...]
#   scripts/test-capped.sh --exec <command> [<args>...]
#
# Overridable env:
#   TEST_CAP_SECS   advisory duration in seconds, or 0 to disable warnings
#   CARGO_TEST_CMD  command to run instead of "cargo test" (for testing this script itself)
#   CARGO_TEST_BUILD_CMD optional build-phase command used by this script's fixtures
#   CODETERM_EMBED_CACHE_DIR shared machine cache for immutable embedding models
#
# Not overridable — always cleared/set before the test command runs:
#   CODETERM_CONPTY_DIR unset unconditionally (an inherited value can point at
#     a real user install; see pty-fork/src/win/psuedocon.rs test isolation)
#   CODETERM_CONPTY_TEST_ISOLATION always exported as 1

set -euo pipefail

EXEC_MODE=0
if [[ "${1:-}" == "--exec" ]]; then
    EXEC_MODE=1
    shift
    [ "$#" -gt 0 ] || {
        printf 'error: --exec requires a command.\n' >&2
        exit 2
    }
fi

TEST_CAP_SECS="${TEST_CAP_SECS:-600}"
if [[ ! "$TEST_CAP_SECS" =~ ^[0-9]+$ ]]; then
    printf 'error: TEST_CAP_SECS must be a non-negative integer.\n' >&2
    exit 2
fi
if [ -n "${CARGO_TEST_CMD:-}" ]; then
    CARGO_TEST_CMD_DEFAULT=0
else
    CARGO_TEST_CMD_DEFAULT=1
fi
CARGO_TEST_CMD="${CARGO_TEST_CMD:-cargo test}"
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
# No awk `exit`: it closes the pipe, SIGPIPEs rustc, and pipefail then aborts the
# whole script at exit 141 with zero output.
HOST_TRIPLE=""
if [ "$EXEC_MODE" -eq 0 ]; then
    HOST_TRIPLE="${CODETERM_TEST_HOST_TRIPLE:-$(rustc -vV | awk -F': ' '/^host:/{print $2}')}"
fi

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

if [ "$EXEC_MODE" -eq 0 ] && [[ -z "$HOST_TRIPLE" ]]; then
    printf 'error: unable to determine Rust host triple.\n' >&2
    exit 2
fi

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
            printf '[test-capped] sccache enabled for the isolated Windows test target (basedir rewriting disabled; see .agents/skills/build/SKILL.md).\n'
        fi
    fi
    return 0
}

ARGS_JOINED=" $* "
if [[ "$ARGS_JOINED" == *" --manifest-path cli/Cargo.toml "* && "$ARGS_JOINED" == *" --lib "* ]]; then
    printf 'error: cli/Cargo.toml is a binary crate and has no --lib test target.\n' >&2
    printf 'hint: use scripts/test-capped.sh --manifest-path cli/Cargo.toml <test-filter>\n' >&2
    exit 2
fi

if [ "$EXEC_MODE" -eq 0 ]; then
    configure_windows_test_target
    if [ "$CARGO_TEST_CMD_DEFAULT" -eq 1 ]; then
        source "$ROOT/scripts/cache-policy.sh"
        ct_configure_sccache
    fi
    configure_embed_cache
    isolate_conpty_test_dir
    if [ "${CODETERM_BUILD_FINGERPRINTS:-0}" = 1 ] && [ -n "${CT_BUILD_ARTIFACT_DIR:-}" ]; then
        node "$ROOT/scripts/build-compiler-inputs.mjs" "$@"
    fi
fi

if [ "$TEST_CAP_SECS" -eq 0 ]; then
    printf '[test-capped] test duration warning disabled (TEST_CAP_SECS=0).\n'
else
    printf '[test-capped] test duration warning threshold: %ss (no termination)\n' "$TEST_CAP_SECS"
fi

ZERO_MATCH_EXIT=97
REQUESTED_FILTER=""
FILTER_PACKAGE=""
FILTER_MANIFEST=""
NO_TESTS_MODE=""

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
            -E|--filter-expr|--filterset) REQUESTED_FILTER="${2:-}"; shift 2 || return 0 ;;
            --filter-expr=*|--filterset=*) REQUESTED_FILTER="${1#*=}"; shift ;;
            --no-tests) NO_TESTS_MODE="${2:-}"; shift 2 || return 0 ;;
            --no-tests=*) NO_TESTS_MODE="${1#*=}"; shift ;;
            --features|-F|--target|--target-dir|--test|--bin|--bench|--example|--exclude|-j|--jobs|--message-format|--profile|--config|--config-file|-Z|--color|--archive-file|--workspace-remap|--binaries-metadata|--cargo-metadata|--cargo-profile|-P|--test-threads|--status-level|--final-status-level|--run-ignored|--retries|--max-fail|--partition)
                shift 2 || return 0 ;;
            -*) shift ;;
            *) REQUESTED_FILTER="$1"; return 0 ;;
        esac
    done
}

# Sums passed tests across libtest result lines and nextest summaries; an
# all-ignored or unmatched selection sums to zero.
passed_test_count() {
    awk '
        { gsub(/\033\[[0-9;]*m/, "") }
        /^[[:space:]]*test result: [A-Za-z]+\. [0-9]+ passed;/ {
            line = $0; sub(/.*test result: [A-Za-z]+\. /, "", line); total += line + 0
        }
        /^[[:space:]]*Summary \[[^]]*\][[:space:]]+[0-9]+ tests? run: [0-9]+ passed/ {
            line = $0; sub(/.* tests? run: /, "", line); total += line + 0
        }
        END { print total + 0 }
    ' "$1"
}

filtered_run_ran_nothing() {
    case "$NO_TESTS_MODE" in pass|warn) return 1 ;; esac
    [ "$(passed_test_count "$1")" -eq 0 ]
}

report_zero_match() {
    local context="${FILTER_PACKAGE:-${FILTER_MANIFEST:-unspecified crate}}"
    printf '\n'
    printf '══════════════════════════════════════════════════════════════════\n'
    printf '  ZERO MATCHES: filter %s selected no tests in %s.\n' "$REQUESTED_FILTER" "$context"
    printf '  No test passed: nothing matched, every match was ignored, or no summary was printed.\n'
    printf '  A filtered run that executed nothing is not a pass.\n'
    printf '  Fix the test name or pick the scope that owns the crate (./build.sh help), or drop the filter.\n'
    printf '══════════════════════════════════════════════════════════════════\n'
}

run_test() {
    local started=$SECONDS status=0 elapsed
    if [ "$EXEC_MODE" -eq 1 ]; then
        "$@" || status=$?
    else
        $CARGO_TEST_CMD "$@" || status=$?
    fi
    elapsed=$((SECONDS - started))
    if [ "$TEST_CAP_SECS" -gt 0 ] && [ "$elapsed" -ge "$TEST_CAP_SECS" ]; then
        printf '[test-capped] WARNING: execution took %ss (advisory %ss); actual exit=%s.\n' "$elapsed" "$TEST_CAP_SECS" "$status" >&2
    fi
    return "$status"
}

run_build() {
    [ "$EXEC_MODE" -eq 0 ] || return 0
    if [ -n "${CARGO_TEST_BUILD_CMD:-}" ]; then
        $CARGO_TEST_BUILD_CMD --no-run "$@"
    elif [ "$CARGO_TEST_CMD_DEFAULT" -eq 1 ]; then
        $CARGO_TEST_CMD --no-run "$@"
    fi
}

if [ "$EXEC_MODE" -eq 0 ]; then
    detect_requested_filter "$@"
    if [ -n "${CARGO_TEST_BUILD_CMD:-}" ] || [ "$CARGO_TEST_CMD_DEFAULT" -eq 1 ]; then
        printf '[test-capped] compiling test targets before execution.\n'
        if run_build "$@"; then
            :
        else
            BUILD_EXIT=$?
            printf '[test-capped] test target compilation failed with exit %s.\n' "$BUILD_EXIT" >&2
            exit "$BUILD_EXIT"
        fi
    fi
fi

TEST_RAW_OUTPUT="${TEST_RAW_OUTPUT:-0}"
if [ "$EXEC_MODE" = 0 ] && [ "$TEST_RAW_OUTPUT" != 1 ] && [ -n "${CT_BUILD_ARTIFACT_DIR:-}" ]; then
    TEST_LOG=$(mktemp "$CT_BUILD_ARTIFACT_DIR/test.XXXXXX.log")
else
    TEST_LOG=$(mktemp)
    trap 'rm -f "$TEST_LOG"' EXIT
fi
capture_test() {
    if [ "$EXEC_MODE" = 1 ] || [ "$TEST_RAW_OUTPUT" = 1 ] || [ -z "${CT_BUILD_ARTIFACT_DIR:-}" ]; then
        run_test "$@" 2>&1 | tee "$TEST_LOG"
    else
        local status=0
        run_test "$@" > "$TEST_LOG" 2>&1 || status=$?
        node "$(dirname "${BASH_SOURCE[0]}")/build-gateway-output.mjs" test-summary "$TEST_LOG" "$status" || true
        return "$status"
    fi
}
if capture_test "$@"; then
    if [ "$EXEC_MODE" -eq 0 ] && [ -n "$REQUESTED_FILTER" ]; then
        if filtered_run_ran_nothing "$TEST_LOG"; then
            report_zero_match
            exit "$ZERO_MATCH_EXIT"
        fi
    fi
    exit 0
else
    CARGO_EXIT=$?
fi

exit "$CARGO_EXIT"
