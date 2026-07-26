#!/usr/bin/env bash
# Run `cargo test` under a process-group timeout.
#
# Kills the WHOLE process group — cargo AND every spawned test binary
# (*/target/*/deps/*) — so no orphans survive the cap.
#
# Exit 124 on timeout (matches GNU timeout); passes through cargo's own exit code otherwise.
#
# Usage:
#   scripts/test-capped.sh [-- <cargo-test-args>...]
#   TEST_CAP_SECS=30 scripts/test-capped.sh -- my_crate::my_mod
#
# Overridable env:
#   TEST_CAP_SECS   seconds before SIGTERM + banner (default 90)
#   CARGO_TEST_CMD  command to run instead of "cargo test" (for testing this script itself)

set -euo pipefail

TEST_CAP_SECS="${TEST_CAP_SECS:-90}"
CARGO_TEST_CMD="${CARGO_TEST_CMD:-cargo test}"
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
HOST_TRIPLE="${CODETERM_TEST_HOST_TRIPLE:-$(rustc -vV | awk -F': ' '/^host:/{print $2; exit}')}"

[[ -n "$HOST_TRIPLE" ]] || {
    printf 'error: unable to determine Rust host triple.\n' >&2
    exit 2
}

configure_windows_test_target() {
    [[ "$HOST_TRIPLE" == *-windows-* ]] || return 0
    case "${CARGO_TARGET_DIR:-}" in
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
            SCCACHE_BASEDIRS="${SCCACHE_BASEDIRS:-$ROOT}"
            export RUSTC_WRAPPER SCCACHE_BASEDIRS
            printf '[test-capped] sccache enabled for the isolated Windows test target.\n'
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
ensure_embed_sidecar

# GNU timeout owns the bounded failure deadline. It preserves the child status
# unless it enforces the deadline, avoiding MSYS job-control group kills.
if timeout --signal=TERM --kill-after=5s "$TEST_CAP_SECS" $CARGO_TEST_CMD "$@"; then
    exit 0
else
    CARGO_EXIT=$?
fi

if [ "$CARGO_EXIT" -eq 124 ]; then
    printf '\n'
    printf '══════════════════════════════════════════════════════════════════\n'
    printf '  DEADLOCK / HANG: cargo test exceeded %ss cap.\n' "$TEST_CAP_SECS"
    printf '  Read the code — do NOT re-run with a bigger cap.\n'
    printf '══════════════════════════════════════════════════════════════════\n'
fi
exit "$CARGO_EXIT"
