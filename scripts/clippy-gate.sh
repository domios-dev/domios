#!/usr/bin/env bash
# Caller: build.sh release-preflight|ci-clippy; scripts test-clippy-gate.sh; skill build, gates, testing
# Single source of truth for the clippy gate — called by .githooks/pre-push
# (integration pushes) and .github/workflows/tests.yml (CI linux job).
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

source scripts/clippy-allow.sh

run_clippy_gate() {
    # --all-targets also lints every target in its non-test build, so it catches test-only dead code alone.
    cargo clippy --manifest-path "$1" --workspace --all-targets --locked -- -D warnings "${CLIPPY_ALLOW[@]}"
}

run_clippy_gate src-tauri/Cargo.toml || exit $?
run_clippy_gate cli/Cargo.toml || exit $?
