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

ensure_embed_sidecar() {
    local triple
    local extension=""
    local sidecar

    triple=$(rustc -vV | awk -F': ' '/^host:/{print $2; exit}')
    [[ -n "$triple" ]] || {
        printf 'error: unable to determine Rust host triple for bundled embed sidecar.\n' >&2
        exit 2
    }
    [[ "$triple" == *-windows-* ]] && extension=".exe"
    sidecar="$ROOT/src-tauri/bundled-embed/codeterm-embed-${triple}${extension}"

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

ensure_embed_sidecar

# Job control: gives each background job its own process group.
# cargo becomes the PGID leader; all test binaries it spawns inherit that PGID.
set -m

TIMEOUT_FLAG=$(mktemp)
trap 'rm -f "$TIMEOUT_FLAG"' EXIT

# shellcheck disable=SC2086
$CARGO_TEST_CMD "$@" &
CARGO_PID=$!

# Watchdog: after the cap, mark the flag, print the banner, and kill the whole group.
(
    sleep "$TEST_CAP_SECS" || true
    printf '1' > "$TIMEOUT_FLAG"
    printf '\n'
    printf '══════════════════════════════════════════════════════════════════\n'
    printf '  DEADLOCK / HANG: cargo test exceeded %ss cap.\n' "$TEST_CAP_SECS"
    printf '  Read the code — do NOT re-run with a bigger cap.\n'
    printf '  Killing process group %s (cargo + spawned test binaries)...\n' "$CARGO_PID"
    printf '══════════════════════════════════════════════════════════════════\n'
    kill -- "-${CARGO_PID}" 2>/dev/null || true
) &
WATCHDOG_PID=$!

# Wait for cargo; capture its exit code without letting set -e abort us.
if wait "$CARGO_PID"; then
    CARGO_EXIT=0
else
    CARGO_EXIT=$?
fi

kill -- "-${WATCHDOG_PID}" 2>/dev/null || true
wait "$WATCHDOG_PID" 2>/dev/null || true

# If the watchdog wrote the flag, report 124 (timeout).
if [ -s "$TIMEOUT_FLAG" ]; then
    exit 124
fi
exit "$CARGO_EXIT"
