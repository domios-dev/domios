#!/usr/bin/env bash
# Caller: build.sh release-preflight; CI tests.yml; scripts test-clippy-gate.sh; skill build, gates, testing
# Single source of truth for the clippy gate — called by .githooks/pre-push
# (integration pushes) and .github/workflows/tests.yml (CI linux job).
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

source scripts/clippy-allow.sh

run_clippy_gate() {
    local manifest="$1"
    shift
    cargo clippy --manifest-path "$manifest" --workspace "$@" -- -D warnings "${CLIPPY_ALLOW[@]}" || return $?
    cargo clippy --manifest-path "$manifest" --workspace --all-targets -- -D warnings "${CLIPPY_ALLOW[@]}" || return $?
}

run_clippy_gate src-tauri/Cargo.toml --lib --bins --examples || exit $?
run_clippy_gate cli/Cargo.toml --bins --examples || exit $?
