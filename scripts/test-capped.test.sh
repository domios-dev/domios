#!/usr/bin/env bash

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

host_triple=$(rustc -vV | awk -F': ' '/^host:/{print $2; exit}')
case "$host_triple" in *-windows-*) embed_ext=.exe ;; *) embed_ext= ;; esac

make_fixture_repo() {
    local repo=$1
    mkdir -p "$repo/scripts"
    cp "$ROOT/scripts/test-capped.sh" "$ROOT/scripts/prepare-embed-stub.js" \
        "$ROOT/scripts/bundle-embed.js" "$repo/scripts/"
}

run_fixture_wrapper() {
    local repo=$1
    local output=$2
    shift 2
    (
        cd "$repo"
        CARGO_TEST_CMD=bash "$repo/scripts/test-capped.sh" -c "$@"
    ) >"$output" 2>&1
}

sidecar_rel="src-tauri/bundled-embed/codeterm-embed-${host_triple}${embed_ext}"

base_repo="$TMPDIR/base-wrapper"
make_fixture_repo "$base_repo"
WRAPPER="$base_repo/scripts/test-capped.sh"

absent_repo="$TMPDIR/absent-sidecar"
make_fixture_repo "$absent_repo"
run_fixture_wrapper "$absent_repo" "$TMPDIR/absent.out" 'test -f "$1"' bash "$sidecar_rel"
[ -f "$absent_repo/$sidecar_rel" ] || {
    printf 'absent-sidecar: wrapper did not create the embed stub\n' >&2
    exit 1
}
[ "$(grep -c '^\[test-capped\] missing bundled embed sidecar; creating test-only stub\.$' "$TMPDIR/absent.out")" -eq 1 ] || {
    printf 'absent-sidecar: expected exactly one attributable heal log line\n' >&2
    exit 1
}

run_fixture_wrapper "$absent_repo" "$TMPDIR/second-run.out" 'test -f "$1"' bash "$sidecar_rel"
! grep -q '^\[test-capped\] missing bundled embed sidecar' "$TMPDIR/second-run.out" || {
    printf 'second-run: existing stub should not be healed again\n' >&2
    exit 1
}

canonical_repo="$TMPDIR/canonical-sidecar"
make_fixture_repo "$canonical_repo"
mkdir -p "$(dirname "$canonical_repo/$sidecar_rel")"
printf 'real-sidecar' >"$canonical_repo/$sidecar_rel"
run_fixture_wrapper "$canonical_repo" "$TMPDIR/canonical.out" 'test "$(cat "$1")" = real-sidecar' bash "$sidecar_rel"
! grep -q '^\[test-capped\] missing bundled embed sidecar' "$TMPDIR/canonical.out" || {
    printf 'canonical-sidecar: real sidecar should be a no-op\n' >&2
    exit 1
}

bundle_repo="$TMPDIR/bundle-stub"
make_fixture_repo "$bundle_repo"
mkdir -p "$bundle_repo/src-tauri/target/release"
: >"$bundle_repo/src-tauri/target/release/codeterm-embed${embed_ext}"
if (
    cd "$bundle_repo"
    env -u CARGO_TARGET_DIR node -e \
        'require("./scripts/bundle-embed.js").bundleEmbed(process.env, process.cwd())'
) >"$TMPDIR/bundle.out" 2>&1; then
    printf 'bundle-stub: empty embed stub was accepted\n' >&2
    exit 1
fi
grep -q 'empty stub, refusing to bundle' "$TMPDIR/bundle.out" || {
    printf 'bundle-stub: expected fail-fast empty-stub error\n' >&2
    exit 1
}

run_prompt_case() {
    local name=$1
    local expected_exit=$2
    local command=$3
    shift 3
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
    ! grep -q 'DEADLOCK / HANG' "$output" || {
        printf '%s: timeout banner printed after prompt exit\n' "$name" >&2
        return 1
    }
}

run_prompt_case fast-success 0 true
run_prompt_case fast-failure 3 bash -c 'exit 3'
run_prompt_case disk-full-propagates 101 bash -c 'printf "os error 112\\n" >&2; exit 101'

no_timeout_bin="$TMPDIR/no-timeout-bin"
mkdir -p "$no_timeout_bin"
printf '%s\n' '#!/usr/bin/env bash' 'printf "default path invoked timeout\\n" >&2' 'exit 99' \
    >"$no_timeout_bin/timeout"
chmod +x "$no_timeout_bin/timeout"

run_uncapped_case() {
    local name=$1
    local expected_exit=$2
    local command=$3
    shift 3
    local actual_exit=0
    local output="$TMPDIR/$name.out"

    if env -u TEST_CAP_SECS PATH="$no_timeout_bin:$PATH" CARGO_TEST_CMD="$command" \
        "$WRAPPER" "$@" >"$output" 2>&1; then
        actual_exit=0
    else
        actual_exit=$?
    fi

    [ "$actual_exit" -eq "$expected_exit" ] || {
        printf '%s: expected exit %s, got %s\n' "$name" "$expected_exit" "$actual_exit" >&2
        return 1
    }
    ! grep -q 'default path invoked timeout' "$output" || {
        printf '%s: uncapped default invoked the timeout runner\n' "$name" >&2
        return 1
    }
    ! grep -q 'DEADLOCK / HANG' "$output" || {
        printf '%s: uncapped child exit was misreported as a timeout\n' "$name" >&2
        return 1
    }
}

run_uncapped_case uncapped-success 0 true
run_uncapped_case uncapped-exit-124 124 bash -c 'exit 124'

windows_target_output="$TMPDIR/windows-target.out"
CODETERM_TEST_HOST_TRIPLE=x86_64-pc-windows-msvc CODETERM_TEST_DEV_HEALTH=healthy \
    CARGO_TEST_CMD=bash "$WRAPPER" -c \
    'case "$CARGO_TARGET_DIR" in */target-check) exit 0;; *) exit 11;; esac' \
    >"$windows_target_output" 2>&1
grep -q 'using isolated Windows test target:' "$windows_target_output" || {
    printf 'windows-target: wrapper did not select the isolated focused-test target\n' >&2
    exit 1
}

inherited_target_output="$TMPDIR/inherited-target.out"
CODETERM_TEST_HOST_TRIPLE=x86_64-pc-windows-msvc \
    CODETERM_TEST_DEV_HEALTH=healthy \
    CARGO_TARGET_DIR='C:\Users\dev\AppData\Local\CodeTerm\build-gateway\targets\local-install\stale' \
    CARGO_TEST_CMD=bash "$WRAPPER" -c \
    'case "$CARGO_TARGET_DIR" in */target-check) exit 0;; *) exit 12;; esac' \
    >"$inherited_target_output" 2>&1
grep -q 'ignoring inherited internal build-gateway target:' "$inherited_target_output" || {
    printf 'inherited-target: wrapper did not reject the internal gateway target\n' >&2
    exit 1
}

explicit_target_output="$TMPDIR/explicit-target.out"
CODETERM_TEST_HOST_TRIPLE=x86_64-pc-windows-msvc CARGO_TARGET_DIR="$TMPDIR/custom-target" \
    CARGO_TEST_CMD=bash "$WRAPPER" -c 'printf "cargo-test-ran\n"' \
    >"$explicit_target_output" 2>&1
grep -q '^cargo-test-ran$' "$explicit_target_output" || {
    printf 'explicit-target: wrapper exited before invoking the test command\n' >&2
    exit 1
}
! grep -q 'using isolated Windows test target' "$explicit_target_output" || {
    printf 'explicit-target: wrapper overrode the caller target directory\n' >&2
    exit 1
}

child_pid_file="$TMPDIR/child.pid"
blocking_fifo="$TMPDIR/blocking-input"
mkfifo "$blocking_fifo"
timeout_exit=0
timeout_output="$TMPDIR/timeout.out"
if TEST_CAP_SECS=2 CARGO_TEST_CMD=bash "$WRAPPER" -c '
    cat "$1" >/dev/null &
    child_pid=$!
    printf "%s" "$child_pid" > "$2"
    wait "$child_pid"
' bash "$blocking_fifo" "$child_pid_file" >"$timeout_output" 2>&1; then
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
    printf 'timeout: blocking child %s survived\n' "$child_pid" >&2
    exit 1
fi
