#!/usr/bin/env bash

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

host_triple=$(rustc -vV | awk -F': ' '/^host:/{print $2}')
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

FAKE_CARGO="$TMPDIR/fake-cargo-test"

# Stands in for `cargo test`: libtest exits 0 even when a filter matches nothing,
# which is the success the wrapper must refuse to pass through.
write_fake_cargo() {
    cat >"$FAKE_CARGO" <<'FAKE'
#!/usr/bin/env bash
set -u
last=""
for arg in "$@"; do last="$arg"; done
case "$last" in
    wrapper_fixture_matching_test)
        printf 'running 1 test\n'
        printf 'test wrapper_fixture_matching_test ... ok\n\n'
        printf 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 41 filtered out\n'
        ;;
    -*|'')
        printf 'running 0 tests\n\n'
        printf 'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n\n'
        printf 'running 42 tests\n\n'
        printf 'test result: ok. 42 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n'
        ;;
    *)
        printf 'running 0 tests\n\n'
        printf 'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 42 filtered out\n'
        ;;
esac
FAKE
    chmod +x "$FAKE_CARGO"
}

run_wrapper_against_fake_cargo() {
    local output=$1
    shift
    write_fake_cargo
    local exit_code=0
    env -u TEST_CAP_SECS CARGO_TARGET_DIR="$TMPDIR/fixture-target" \
        CARGO_TEST_CMD="$FAKE_CARGO" "$WRAPPER" "$@" >"$output" 2>&1 || exit_code=$?
    printf '%s' "$exit_code"
}

filtered_zero_match_fails_closed() {
    local output="$TMPDIR/filtered-zero-match.out"
    local exit_code
    exit_code=$(run_wrapper_against_fake_cargo "$output" \
        --manifest-path src-tauri/Cargo.toml -p codeterm-plan --lib no_such_test_name)
    [ "$exit_code" -eq 97 ] || {
        printf 'filtered_zero_match_fails_closed: expected exit 97, got %s\n' "$exit_code" >&2
        exit 1
    }
    grep -q 'ZERO MATCHES: filter no_such_test_name selected no tests in codeterm-plan\.' "$output" || {
        printf 'filtered_zero_match_fails_closed: diagnostic did not name the filter and crate\n' >&2
        exit 1
    }
    ! grep -q 'DEADLOCK / HANG' "$output" || {
        printf 'filtered_zero_match_fails_closed: zero match was misreported as a timeout\n' >&2
        exit 1
    }
}

filtered_match_remains_successful() {
    local output="$TMPDIR/filtered-match.out"
    local exit_code
    exit_code=$(run_wrapper_against_fake_cargo "$output" \
        --manifest-path src-tauri/Cargo.toml -p codeterm-plan --lib wrapper_fixture_matching_test)
    [ "$exit_code" -eq 0 ] || {
        printf 'filtered_match_remains_successful: expected exit 0, got %s\n' "$exit_code" >&2
        exit 1
    }
    ! grep -q 'ZERO MATCHES' "$output" || {
        printf 'filtered_match_remains_successful: a matching filter was failed closed\n' >&2
        exit 1
    }
    grep -q '^test wrapper_fixture_matching_test \.\.\. ok$' "$output" || {
        printf 'filtered_match_remains_successful: wrapper swallowed the runner output\n' >&2
        exit 1
    }
}

full_run_passes_through() {
    local output="$TMPDIR/full-run.out"
    local exit_code
    exit_code=$(run_wrapper_against_fake_cargo "$output" \
        --manifest-path src-tauri/Cargo.toml -p codeterm-plan --lib)
    [ "$exit_code" -eq 0 ] || {
        printf 'full_run_passes_through: expected exit 0, got %s\n' "$exit_code" >&2
        exit 1
    }
    ! grep -q 'ZERO MATCHES' "$output" || {
        printf 'full_run_passes_through: unfiltered run was treated as a zero-match filter\n' >&2
        exit 1
    }
    grep -q '^running 0 tests$' "$output" || {
        printf 'full_run_passes_through: fixture did not exercise a zero-test binary\n' >&2
        exit 1
    }
    grep -q '^running 42 tests$' "$output" || {
        printf 'full_run_passes_through: fixture did not exercise a populated binary\n' >&2
        exit 1
    }
    grep -q '^\[test-capped\] test execution cap: 600s$' "$output" || {
        printf 'full_run_passes_through: default focused-test cap was not enabled\n' >&2
        exit 1
    }
}

if [ "$#" -gt 0 ]; then
    for requested_case in "$@"; do
        case "$requested_case" in
            filtered_zero_match_fails_closed|filtered_match_remains_successful|full_run_passes_through)
                "$requested_case"
                ;;
            *)
                printf 'unknown case: %s\n' "$requested_case" >&2
                exit 2
                ;;
        esac
        printf 'ok %s\n' "$requested_case"
    done
    exit 0
fi

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

run_cap_disabled_case() {
    local name=$1
    local expected_exit=$2
    local command=$3
    shift 3
    local actual_exit=0
    local output="$TMPDIR/$name.out"

    if TEST_CAP_SECS=0 PATH="$no_timeout_bin:$PATH" CARGO_TEST_CMD="$command" \
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
        printf '%s: disabled cap invoked the timeout runner\n' "$name" >&2
        return 1
    }
    ! grep -q 'DEADLOCK / HANG' "$output" || {
        printf '%s: uncapped child exit was misreported as a timeout\n' "$name" >&2
        return 1
    }
}

run_cap_disabled_case disabled-cap-success 0 true
run_cap_disabled_case disabled-cap-exit-124 124 bash -c 'exit 124'

shared_home="$TMPDIR/shared-home"
ephemeral_data="$TMPDIR/ephemeral-data"
mkdir -p "$shared_home" "$ephemeral_data"
HOME="$shared_home" CODETERM_DATA_DIR="$ephemeral_data" CARGO_TEST_CMD=bash \
    "$WRAPPER" -c 'test "$CODETERM_EMBED_CACHE_DIR" = "$HOME/.codeterm/fastembed_cache"' \
    >"$TMPDIR/shared-embed-cache.out" 2>&1
grep -q '^\[test-capped\] shared embed cache:' "$TMPDIR/shared-embed-cache.out" || {
    printf 'shared-embed-cache: wrapper did not disclose the selected machine cache\n' >&2
    exit 1
}

explicit_embed_cache="$TMPDIR/explicit-embed-cache"
HOME="$shared_home" CODETERM_EMBED_CACHE_DIR="$explicit_embed_cache" CARGO_TEST_CMD=bash \
    "$WRAPPER" -c 'test "$CODETERM_EMBED_CACHE_DIR" = "$1"' bash "$explicit_embed_cache" \
    >"$TMPDIR/explicit-embed-cache.out" 2>&1
! grep -q '^\[test-capped\] shared embed cache:' "$TMPDIR/explicit-embed-cache.out" || {
    printf 'explicit-embed-cache: wrapper replaced the caller override\n' >&2
    exit 1
}

# A test process launched from a CodeTerm tab inherits CODETERM_CONPTY_DIR
# pointing at the real installed conpty bundle. The wrapper must refuse to let
# that ambient value reach the test command at all, not merely usually strip it.
production_conpty_dir='C:\Users\User\AppData\Local\CodeTerm\conpty\x64'
HOME="$shared_home" CODETERM_CONPTY_DIR="$production_conpty_dir" CARGO_TEST_CMD=bash \
    "$WRAPPER" -c 'printf "resolved=%s\n" "${CODETERM_CONPTY_DIR:-<unset>}"' \
    >"$TMPDIR/conpty-isolation.out" 2>&1
! grep -qF "resolved=$production_conpty_dir" "$TMPDIR/conpty-isolation.out" || {
    printf 'conpty-isolation: wrapper let an ambient production CODETERM_CONPTY_DIR reach the test command\n' >&2
    exit 1
}
grep -qF 'resolved=<unset>' "$TMPDIR/conpty-isolation.out" || {
    printf 'conpty-isolation: expected CODETERM_CONPTY_DIR to be unset for the test command\n' >&2
    exit 1
}

# Fail-closed: the isolation marker is exported even when no CODETERM_CONPTY_DIR
# was ever present in the parent env — never conditioned on the leaked
# variable happening to exist, so pty-fork's fork-side gate always has it.
HOME="$shared_home" CARGO_TEST_CMD=bash \
    "$WRAPPER" -c 'printf "marker=%s\n" "${CODETERM_CONPTY_TEST_ISOLATION:-<unset>}"' \
    >"$TMPDIR/conpty-isolation-marker.out" 2>&1
grep -qF 'marker=1' "$TMPDIR/conpty-isolation-marker.out" || {
    printf 'conpty-isolation-marker: wrapper did not export CODETERM_CONPTY_TEST_ISOLATION\n' >&2
    exit 1
}

windows_sidecar="$base_repo/src-tauri/bundled-embed/codeterm-embed-x86_64-pc-windows-msvc.exe"
mkdir -p "$(dirname "$windows_sidecar")"
printf 'real-sidecar' >"$windows_sidecar"

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
    CARGO_TARGET_DIR='C:\Users\User\AppData\Local\CodeTerm\build-gateway\targets\local-install\stale' \
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

slow_build_command="$TMPDIR/slow-build-command"
cat >"$slow_build_command" <<'SLOW_BUILD'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$@" > "$BUILD_ARGS_FILE"
TEST_CAP_SECS=5 "$BUILD_WRAPPER" --exec bash -c 'printf started > "$1"' bash "$BUILD_STARTED_FIFO" >/dev/null
TEST_CAP_SECS=10 "$BUILD_WRAPPER" --exec cat "$BUILD_RELEASE_FIFO" >/dev/null
printf 'slow-build-completed\n'
SLOW_BUILD
chmod +x "$slow_build_command"
build_started_fifo="$TMPDIR/build-started"
build_release_fifo="$TMPDIR/build-release"
build_probe_fifo="$TMPDIR/build-probe"
mkfifo "$build_started_fifo" "$build_release_fifo" "$build_probe_fifo"
build_args_file="$TMPDIR/build-args"
build_wrapper_output="$TMPDIR/slow-build-wrapper.out"
build_started_output="$TMPDIR/build-started.out"
TEST_CAP_SECS=5 "$WRAPPER" --exec cat "$build_started_fifo" >"$build_started_output" 2>&1 &
build_started_reader_pid=$!
TEST_CAP_SECS=1 CARGO_TEST_CMD=true CARGO_TEST_BUILD_CMD="$slow_build_command" \
    BUILD_WRAPPER="$WRAPPER" BUILD_ARGS_FILE="$build_args_file" BUILD_STARTED_FIFO="$build_started_fifo" \
    BUILD_RELEASE_FIFO="$build_release_fifo" "$WRAPPER" --lib \
    >"$build_wrapper_output" 2>&1 &
slow_build_wrapper_pid=$!
if wait "$build_started_reader_pid"; then
    build_started_exit=0
else
    build_started_exit=$?
fi
if TEST_CAP_SECS=2 "$WRAPPER" --exec cat "$build_probe_fifo" \
    >"$TMPDIR/build-probe.out" 2>&1; then
    build_probe_exit=0
else
    build_probe_exit=$?
fi
if TEST_CAP_SECS=5 "$WRAPPER" --exec bash -c 'printf release > "$1"' bash "$build_release_fifo" \
    >"$TMPDIR/build-release.out" 2>&1; then
    build_release_exit=0
else
    build_release_exit=$?
fi
if wait "$slow_build_wrapper_pid"; then
    slow_build_exit=0
else
    slow_build_exit=$?
fi
[ "$build_started_exit" -eq 0 ] || {
    printf 'slow-build: build phase did not signal its start, exit %s\n' "$build_started_exit" >&2
    exit 1
}
[ "$build_probe_exit" -eq 124 ] || {
    printf 'slow-build: bounded probe expected exit 124, got %s\n' "$build_probe_exit" >&2
    exit 1
}
[ "$build_release_exit" -eq 0 ] || {
    printf 'slow-build: could not release the build event, exit %s\n' "$build_release_exit" >&2
    exit 1
}
[ "$slow_build_exit" -eq 0 ] || {
    printf 'slow-build: build lasting beyond the execution cap failed with exit %s\n' "$slow_build_exit" >&2
    exit 1
}
grep -Fxq -- '--no-run' "$build_args_file" || {
    printf 'slow-build: preflight command did not receive --no-run\n' >&2
    exit 1
}
grep -Fxq -- '--lib' "$build_args_file" || {
    printf 'slow-build: preflight command did not receive the Cargo test arguments\n' >&2
    exit 1
}
grep -q '^slow-build-completed$' "$build_wrapper_output" || {
    printf 'slow-build: preflight output did not complete before test execution\n' >&2
    exit 1
}
! grep -q 'DEADLOCK / HANG' "$build_wrapper_output" || {
    printf 'slow-build: execution cap incorrectly included the build phase\n' >&2
    exit 1
}

child_pid_file="$TMPDIR/child.pid"
blocking_fifo="$TMPDIR/blocking-input"
mkfifo "$blocking_fifo"
timeout_exit=0
timeout_output="$TMPDIR/timeout.out"
if TEST_CAP_SECS=2 CARGO_TEST_CMD=bash "$WRAPPER" -c '
    printf "running 1 test\n" >&2
    printf "test service::pty_io::tests::provider_update_and_trust_frames_refuse_initial_submit has been running for over 60 seconds\n" >&2
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
grep -q 'Tests still running at the cap:' "$timeout_output" || {
    printf 'timeout: missing active-test summary\n' >&2
    exit 1
}
grep -q 'service::pty_io::tests::provider_update_and_trust_frames_refuse_initial_submit (over 60 seconds)' "$timeout_output" || {
    printf 'timeout: active test name was not echoed\n' >&2
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

filtered_zero_match_fails_closed
filtered_match_remains_successful
full_run_passes_through
